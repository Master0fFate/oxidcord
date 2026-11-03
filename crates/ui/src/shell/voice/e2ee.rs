use std::rc::Rc;

use gpui::{
	AnyElement, App, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _, div,
	prelude::FluentBuilder as _,
};
use gpui_component::popover::Popover;
use protocol::Snowflake;
use state::VoiceSession;

use super::widgets::SignalTrigger;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::theme::Theme;

const CODE_GROUP_DIGITS: usize = 5;
const CODE_GROUPS_PER_ROW: usize = 3;

pub struct E2eeView {
	failed: bool,
	code_rows: Vec<String>,
	members: Vec<SharedString>,
}

impl E2eeView {
	pub fn new(session: &VoiceSession, name_of: impl Fn(Snowflake) -> SharedString) -> Option<Rc<Self>> {
		if !session.e2ee_failed && session.e2ee.is_none() {
			return None;
		}
		let info = session.e2ee.as_ref();
		Some(Rc::new(Self {
			failed: session.e2ee_failed,
			code_rows: info
				.and_then(|info| info.privacy_code.as_deref())
				.map(code_rows)
				.unwrap_or_default(),
			members: info
				.map(|info| info.members.iter().map(|member| name_of(*member)).collect())
				.unwrap_or_default(),
		}))
	}
}

fn code_rows(code: &str) -> Vec<String> {
	let groups: Vec<&str> = code
		.as_bytes()
		.chunks(CODE_GROUP_DIGITS)
		.filter_map(|group| std::str::from_utf8(group).ok())
		.collect();
	groups
		.chunks(CODE_GROUPS_PER_ROW)
		.map(|row| row.join(" "))
		.collect()
}

pub fn badge(view: Rc<E2eeView>, cx: &App) -> impl IntoElement {
	let theme = Theme::current(cx);
	let (icon, color) = if view.failed {
		(IconKind::Warning, theme.danger)
	} else {
		(IconKind::Lock, theme.success)
	};
	Popover::new("voice-e2ee")
		.trigger(SignalTrigger::new(icon, color).compact())
		.content(move |_, _, cx| card(&view, cx))
}

fn card(view: &E2eeView, cx: &App) -> AnyElement {
	let theme = Theme::current(cx);
	let title = if view.failed {
		cx.t("voice-e2ee-failed")
	} else {
		cx.t("voice-e2ee-on")
	};
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.w(theme.voice_menu_width)
		.child(
			div()
				.text_size(theme.font_sm)
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(if view.failed {
					theme.danger
				} else {
					theme.header_primary
				})
				.child(title),
		)
		.when(!view.code_rows.is_empty(), |card| {
			card.child(section(cx.t("voice-e2ee-code"), &theme))
				.children(view.code_rows.iter().map(|row| {
					div()
						.text_size(theme.font_md)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(theme.text_normal)
						.child(row.clone())
				}))
		})
		.when(!view.members.is_empty(), |card| {
			card.child(section(cx.t("voice-e2ee-members"), &theme))
				.children(view.members.iter().map(|name| {
					div()
						.truncate()
						.text_size(theme.font_sm)
						.text_color(theme.text_normal)
						.child(name.clone())
				}))
		})
		.into_any_element()
}

fn section(label: SharedString, theme: &Theme) -> impl IntoElement {
	div()
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_secondary)
		.child(label.to_uppercase())
}
