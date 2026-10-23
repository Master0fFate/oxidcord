use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gpui::{AnyElement, App, FontWeight, Global, IntoElement as _, ParentElement as _, Styled as _, div};
use gpui_component::Disableable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use session::Command;
use state::State;

use super::data_handle;
use crate::components::Avatar;
use crate::i18n::{ArgValue, Translate as _};
use crate::theme::Theme;

const PREFIXES: [&str; 3] = ["discord.gg/", "discord.com/invite/", "discordapp.com/invite/"];
const MAX_CARDS: usize = 2;
const CODE_MIN: usize = 2;
const CODE_MAX: usize = 32;

#[derive(Default, Clone)]
pub struct InviteRequests(pub Rc<RefCell<HashSet<String>>>);

impl Global for InviteRequests {}

pub fn codes(content: &str) -> Vec<String> {
	let lowered = content.to_ascii_lowercase();
	let mut found: Vec<String> = Vec::new();
	for prefix in PREFIXES {
		let mut from = 0;
		while let Some(offset) = lowered[from..].find(prefix) {
			let start = from + offset + prefix.len();
			let code: String = content[start..]
				.chars()
				.take_while(|character| {
					character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
				})
				.take(CODE_MAX)
				.collect();
			from = start;
			if (CODE_MIN..=CODE_MAX).contains(&code.len()) && !found.contains(&code) {
				found.push(code);
			}
		}
	}
	found.truncate(MAX_CARDS);
	found
}

fn request_once(code: &str, cx: &App) {
	let Some(requests) = cx.try_global::<InviteRequests>() else {
		return;
	};
	if requests.0.borrow_mut().insert(code.to_owned()) {
		data_handle(cx).read(cx).send(Command::ResolveInvite {
			code: code.to_owned(),
		});
	}
}

pub fn cards(content: &str, state: &State, theme: &Theme, cx: &App) -> Vec<AnyElement> {
	if !content.contains('/') {
		return Vec::new();
	}
	codes(content)
		.into_iter()
		.filter_map(|code| card(&code, state, theme, cx))
		.collect()
}

fn card(code: &str, state: &State, theme: &Theme, cx: &App) -> Option<AnyElement> {
	request_once(code, cx);
	let preview = state.invite_preview(code).cloned()?;
	let joined = preview.guild_id.is_some_and(|id| state.guild(id).is_some());
	let icon = preview
		.guild_id
		.zip(preview.guild_icon.as_deref())
		.map(|(id, hash)| media::cdn::guild_icon_url(id.get(), hash, 128));
	let avatar = Avatar::new(preview.guild_name.clone()).size(theme.invite_card_icon);
	let avatar = match icon {
		Some(url) => avatar.url(url),
		None => avatar,
	};
	let accept = code.to_owned();
	let key = format!("invite-join-{code}");
	Some(
		div()
			.flex()
			.flex_col()
			.gap(theme.space_12)
			.mt(theme.space_4)
			.p(theme.space_16)
			.w(theme.invite_card_width)
			.max_w_full()
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_secondary)
			.child(
				div()
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_secondary)
					.child(
						cx.t(if joined {
							"invite-card-joined-caption"
						} else {
							"invite-card-caption"
						})
						.to_uppercase(),
					),
			)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_16)
					.child(avatar)
					.child(
						div()
							.flex()
							.flex_col()
							.flex_1()
							.min_w_0()
							.gap(theme.space_4)
							.child(
								div()
									.truncate()
									.text_size(theme.font_md)
									.font_weight(FontWeight::BOLD)
									.text_color(theme.header_primary)
									.child(preview.guild_name.clone()),
							)
							.child(
								div()
									.flex()
									.items_center()
									.gap(theme.space_12)
									.text_size(theme.font_sm)
									.text_color(theme.text_muted)
									.child(stat(
										theme.status_online,
										preview.online_count,
										"invite-card-online",
										theme,
										cx,
									))
									.child(stat(
										theme.status_offline,
										preview.member_count,
										"invite-card-members",
										theme,
										cx,
									)),
							),
					)
					.child(
						Button::new(gpui::SharedString::from(key))
							.success()
							.disabled(joined)
							.label(cx.t(if joined {
								"invite-card-joined"
							} else {
								"invite-card-join"
							}))
							.on_click(move |_, _, cx| {
								data_handle(cx)
									.read(cx)
									.send(Command::AcceptInvite { code: accept.clone() });
							}),
					),
			)
			.into_any_element(),
	)
}

pub(super) fn stat(color: gpui::Hsla, count: i64, key: &str, theme: &Theme, cx: &App) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_4)
		.child(div().size(theme.space_8).rounded_full().bg(color))
		.child(cx.t_args(key, &[("count", ArgValue::from(count))]))
		.into_any_element()
}
