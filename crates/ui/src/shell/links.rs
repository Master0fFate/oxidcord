use std::rc::Rc;

use gpui::{App, ParentElement as _, Styled as _, Window, div};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dialog::DialogFooter;
use plugin_host::api::model::LinkVerdict;
use url::Url;

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

fn matches_domain(host: &str, domain: &str) -> bool {
	host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
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

pub fn open_link(url: &str, backend: &Rc<SettingsBackend>, window: &mut Window, cx: &mut App) {
	if !plugins::link_hook_declared(cx) {
		open_checked(url, backend, window, cx);
		return;
	}
	let (original, backend) = (url.to_owned(), backend.clone());
	plugins::defer_link(
		url,
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
