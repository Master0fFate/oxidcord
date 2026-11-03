use gpui::{
	AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, div,
	prelude::FluentBuilder as _,
};
use gpui_component::progress::Progress;
use protocol::Nonce;
use state::{PendingFile, PendingUpload};

use super::{RowAction, RowEnv, nonce_key};
use crate::components::IconButton;
use crate::format;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

fn file_row(file: &PendingFile, index: usize, key: u64, theme: &Theme) -> AnyElement {
	div()
		.id(("upload-file", key ^ index as u64))
		.flex()
		.items_center()
		.gap(theme.space_12)
		.child(
			Icon::new(IconKind::File)
				.size(rem(32.))
				.color(theme.interactive_normal),
		)
		.child(
			div()
				.flex()
				.flex_col()
				.flex_1()
				.min_w_0()
				.child(
					div()
						.truncate()
						.text_size(theme.font_md)
						.text_color(theme.text_normal)
						.child(file.name.clone()),
				)
				.child(
					div()
						.text_size(theme.font_tiny)
						.text_color(theme.text_muted)
						.child(format::file_size(file.size)),
				),
		)
		.into_any_element()
}

pub fn card(upload: &PendingUpload, nonce: &Nonce, active: bool, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let key = nonce_key(nonce);
	let total: u64 = upload.files.iter().map(|file| file.size).sum();
	let percent = if total == 0 {
		0.
	} else {
		upload.uploaded as f32 / total as f32 * 100.
	};
	let dispatch = env.dispatch.clone();
	let nonce = nonce.clone();
	div()
		.id(("upload", key))
		.mt(theme.space_4)
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.w(rem(432.))
		.max_w_full()
		.p(theme.space_12)
		.rounded(theme.radius_md)
		.border_1()
		.border_color(theme.code_border)
		.bg(theme.background_secondary)
		.children(
			upload
				.files
				.iter()
				.enumerate()
				.map(|(index, file)| file_row(file, index, key, theme)),
		)
		.when(active, |card| {
			card.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(
						div()
							.flex_1()
							.child(Progress::new(("upload-progress", key)).value(percent)),
					)
					.child(
						IconButton::new(("upload-cancel", key), IconKind::Close)
							.icon_size(rem(20.))
							.hit_size(rem(28.))
							.tooltip(cx.t("upload-cancel"))
							.on_click(move |_, window, cx| {
								dispatch(RowAction::Discard(nonce.clone()), window, cx);
							}),
					),
			)
		})
		.into_any_element()
}
