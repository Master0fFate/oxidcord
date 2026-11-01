use std::rc::Rc;

use gpui::{App, ParentElement as _, Styled as _, Window, div};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dialog::DialogFooter;
use plugin_host::api::model::LinkVerdict;
use protocol::Snowflake;
use session::Command;
use url::Url;

use super::community::{CommunityHandle, add_server};
use super::plugins::LinkOrigin;
use super::{SettingsBackend, plugins};
use crate::i18n::{ArgValue, Translate as _};
use crate::theme::{Theme, rem};

const ALLOWED_HOSTS: [&str; 12] = [
	"discord.com",
	"discordapp.com",
	"discordapp.net",
	"github.com",
	"youtube.com",
	"youtu.be",
	"twitter.com",
	"x.com",
	"reddit.com",
	"wikipedia.org",
	"spotify.com",
	"twitch.tv",
];

const DISCORD_HOSTS: [&str; 2] = ["discord.com", "discordapp.com"];
const INVITE_LENGTH: std::ops::RangeInclusive<usize> = 2..=32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscordLink {
	Invite(String),
	Channel {
		guild: Option<Snowflake>,
		channel: Snowflake,
		message: Option<Snowflake>,
	},
}

fn matches_domain(host: &str, domain: &str) -> bool {
	host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
}

fn invite_code(code: &str) -> Option<String> {
	let valid = INVITE_LENGTH.contains(&code.len())
		&& code
			.chars()
			.all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
	valid.then(|| code.to_owned())
}

fn snowflake(text: &str) -> Option<Snowflake> {
	text.parse::<u64>().ok().filter(|id| *id != 0).map(Snowflake)
}

fn path_segments(parsed: &Url) -> Vec<&str> {
	parsed
		.path_segments()
		.map(|segments| segments.filter(|part| !part.is_empty()).collect())
		.unwrap_or_default()
}

pub fn discord_link(url: &str) -> Option<DiscordLink> {
	let parsed = Url::parse(url).ok()?;
	if !matches!(parsed.scheme(), "http" | "https") {
		return None;
	}
	let host = parsed.host_str()?.to_ascii_lowercase();
	let parts = path_segments(&parsed);
	if matches_domain(&host, "discord.gg") {
		return invite_code(parts.first()?).map(DiscordLink::Invite);
	}
	if !DISCORD_HOSTS.iter().any(|domain| matches_domain(&host, domain)) {
		return None;
	}
	match parts.as_slice() {
		["invite", code, ..] => invite_code(code).map(DiscordLink::Invite),
		["channels", guild, channel, rest @ ..] => {
			let channel = snowflake(channel)?;
			let guild = match *guild {
				"@me" => None,
				id => Some(snowflake(id)?),
			};
			let message = rest.first().and_then(|id| snowflake(id));
			Some(DiscordLink::Channel {
				guild,
				channel,
				message,
			})
		}
		_ => None,
	}
}

fn open_internal(url: &str, window: &mut Window, cx: &mut App) -> bool {
	let Some(link) = discord_link(url) else {
		return false;
	};
	let Some(handle) = cx.try_global::<CommunityHandle>().cloned() else {
		return false;
	};
	let data = handle.data.clone();
	match link {
		DiscordLink::Invite(code) => add_server::open_with(&code, window, cx),
		DiscordLink::Channel {
			guild,
			channel,
			message,
		} => {
			let known = {
				let data = data.read(cx);
				let state = data.state();
				state.channel(channel).is_some()
			};
			if !known {
				return false;
			}
			let guild = guild.or_else(|| data.read(cx).state().channel_guild(channel));
			let command = match message {
				Some(message_id) => Command::JumpToMessage {
					channel_id: channel,
					message_id,
				},
				None => Command::SelectChannel {
					guild_id: guild,
					channel_id: channel,
				},
			};
			data.read(cx).send(command);
		}
	}
	true
}

fn trusted(host: &str, extra: &[String]) -> bool {
	ALLOWED_HOSTS
		.iter()
		.copied()
		.chain(extra.iter().map(String::as_str))
		.any(|domain| matches_domain(host, domain))
}

fn launch(url: &str) {
	if let Err(error) = platform::open::open_url(url) {
		tracing::warn!(%error, "could not open the link");
	}
}

pub fn open_link(
	url: &str,
	origin: LinkOrigin,
	backend: &Rc<SettingsBackend>,
	window: &mut Window,
	cx: &mut App,
) {
	if open_internal(url, window, cx) {
		return;
	}
	if !plugins::link_hook_declared(cx) {
		open_checked(url, backend, window, cx);
		return;
	}
	let (original, backend) = (url.to_owned(), backend.clone());
	plugins::defer_link(
		url,
		origin,
		move |verdict, window, cx| match verdict {
			LinkVerdict::Pass => open_checked(&original, &backend, window, cx),
			LinkVerdict::Open(replacement) => open_replacement(&replacement, &backend, window, cx),
			LinkVerdict::Consume => {}
		},
		window,
		cx,
	);
}

fn open_replacement(url: &str, backend: &Rc<SettingsBackend>, window: &mut Window, cx: &mut App) {
	if url.starts_with("http://") || url.starts_with("https://") {
		open_checked(url, backend, window, cx);
	} else if let Err(error) = platform::open::open_plugin_url(url) {
		tracing::warn!(%error, "could not open the plugin link");
	}
}

fn open_checked(url: &str, backend: &Rc<SettingsBackend>, window: &mut Window, cx: &mut App) {
	let Ok(parsed) = Url::parse(url) else {
		return;
	};
	if !matches!(parsed.scheme(), "http" | "https") {
		return;
	}
	let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
		return;
	};
	if trusted(&host, &(backend.read)().trusted_link_hosts) {
		launch(url);
		return;
	}
	let target = url.to_owned();
	let backend = backend.clone();
	window.open_dialog(cx, move |dialog, window, cx| {
		let theme = Theme::current(cx);
		let open = {
			let target = target.clone();
			move |window: &mut Window, cx: &mut App| {
				launch(&target);
				window.close_dialog(cx);
			}
		};
		let trust = {
			let target = target.clone();
			let host = host.clone();
			let backend = backend.clone();
			move |window: &mut Window, cx: &mut App| {
				(backend.write)(&|settings| {
					if !settings.trusted_link_hosts.contains(&host) {
						settings.trusted_link_hosts.push(host.clone());
					}
				});
				launch(&target);
				window.close_dialog(cx);
			}
		};
		dialog
			.title(cx.t("link-title"))
			.w(rem(460.).to_pixels(window.rem_size()))
			.close_button(false)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.child(cx.t("link-body"))
					.child(
						div()
							.p(theme.space_8)
							.rounded(theme.radius_sm)
							.bg(theme.background_tertiary)
							.text_color(theme.link)
							.child(target.clone()),
					),
			)
			.footer(
				DialogFooter::new()
					.child(
						Button::new("link-cancel")
							.label(cx.t("action-cancel"))
							.on_click(|_, window, cx| window.close_dialog(cx)),
					)
					.child(
						Button::new("link-trust")
							.label(cx.t_args("link-trust", &[("host", ArgValue::from(host.clone()))]))
							.on_click({
								let trust = trust.clone();
								move |_, window, cx| trust(window, cx)
							}),
					)
					.child(
						Button::new("link-open")
							.primary()
							.label(cx.t("link-open"))
							.on_click({
								let open = open.clone();
								move |_, window, cx| open(window, cx)
							}),
					),
			)
	});
}
