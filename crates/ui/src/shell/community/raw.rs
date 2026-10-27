use gpui::{
	App, AppContext as _, Context, EventEmitter, InteractiveElement as _, IntoElement, ParentElement as _,
	Render, SharedString, Styled as _, Window, div,
};
use gpui_component::scroll::ScrollableElement as _;

use super::modal::{self, Close};
use crate::components::dialog_footer;
use crate::i18n::Translate as _;
use crate::theme::{Theme, rem};

const RAW_HEIGHT: f32 = 420.;

pub struct RawView {
	json: SharedString,
}

impl EventEmitter<Close> for RawView {}

pub fn open(window: &mut Window, cx: &mut App, title: SharedString, value: &impl serde::Serialize) {
	let json = match serde_json::to_string_pretty(value) {
		Ok(json) => json,
		Err(error) => {
			tracing::warn!(%error, "could not serialize the raw view");
			return;
		}
	};
	let view = cx.new(|_| RawView { json: json.into() });
	let width = cx.global::<Theme>().modal_width_md;
	modal::open(window, cx, title, width, &view, None);
}

impl Render for RawView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let json = self.json.clone();
		let weak = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(
				div()
					.id("raw-json")
					.max_h(rem(RAW_HEIGHT))
					.px(theme.space_12)
					.py(theme.space_8)
					.rounded(theme.radius_sm)
					.bg(theme.code_background)
					.border_1()
					.border_color(theme.code_border)
					.font_family(theme.code_font.clone())
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.whitespace_normal()
					.child(self.json.clone())
					.overflow_y_scrollbar(),
			)
			.child(dialog_footer(
				cx.t("action-close"),
				cx.t("menu-copy"),
				false,
				true,
				&theme,
				move |_, cx| {
					weak.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, _| {
					if let Err(error) = platform::clipboard::write_text(&json) {
						tracing::warn!(%error, "could not copy the raw view");
					}
				},
			))
	}
}
