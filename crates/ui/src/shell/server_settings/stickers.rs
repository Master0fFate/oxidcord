use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, FontWeight, IntoElement, ObjectFit,
	ParentElement as _, Render, SharedString, Styled as _, StyledImage as _, Subscription, Window, div, img,
	prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{Disableable as _, WindowExt as _};
use protocol::{Snowflake, Sticker};
use session::Command;

use super::{kit, sticker_edit};
use crate::components::{IconButton, field};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::shell::community::art::{cover, sticker_url};
use crate::shell::community::files::{Picked, pick_image};
use crate::theme::Theme;

const SLOTS: usize = 5;
const NAME_MIN: usize = 2;
const NAME_MAX: usize = 30;

pub struct StickersPage {
	data: Entity<Data>,
	guild: Snowflake,
	upload: Option<Picked>,
	name: Entity<InputState>,
	description: Entity<InputState>,
	tags: Entity<InputState>,
	pending_name: Option<String>,
	_subscriptions: Vec<Subscription>,
}

fn stem(name: &str) -> String {
	name.rsplit_once('.')
		.map_or(name, |(stem, _)| stem)
		.chars()
		.take(NAME_MAX)
		.collect()
}

impl StickersPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		data.read(cx).send(Command::LoadGuildStickers { guild_id: guild });
		let name = cx.new(|cx| InputState::new(window, cx));
		let description_placeholder = cx.t("stickers-description-placeholder");
		let description = cx.new(|cx| InputState::new(window, cx).placeholder(description_placeholder));
		let tags_placeholder = cx.t("stickers-tags-placeholder");
		let tags = cx.new(|cx| InputState::new(window, cx).placeholder(tags_placeholder));
		let subscription = cx.subscribe_in(&name, window, |_, _, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		});
		Self {
			data,
			guild,
			upload: None,
			name,
			description,
			tags,
			pending_name: None,
			_subscriptions: vec![subscription],
		}
	}

	fn stickers(&self, cx: &App) -> Vec<Sticker> {
		self.data
			.read(cx)
			.state()
			.guild_stickers(self.guild)
			.map(<[Sticker]>::to_vec)
			.unwrap_or_default()
	}

	fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let name: String = self.name.read(cx).value().trim().chars().take(NAME_MAX).collect();
		if name.chars().count() < NAME_MIN {
			return;
		}
		let Some(picked) = self.upload.take() else {
			return;
		};
		self.data.read(cx).send(Command::CreateSticker {
			guild_id: self.guild,
			name,
			description: self.description.read(cx).value().trim().to_owned(),
			tags: self.tags.read(cx).value().trim().to_owned(),
			file_png_or_apng: picked.bytes,
			filename: picked.name,
		});
		for input in [&self.name, &self.description, &self.tags] {
			input.update(cx, |state, cx| state.set_value("", window, cx));
		}
		cx.notify();
	}

	fn confirm_delete(&mut self, id: Snowflake, name: String, window: &mut Window, cx: &mut Context<Self>) {
		let data = self.data.clone();
		let guild = self.guild;
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			alert
				.title(cx.t("stickers-delete-title"))
				.description(cx.t_args("stickers-delete-body", &[("name", ArgValue::from(name.clone()))]))
				.confirm()
				.ok_text(cx.t("menu-delete"))
				.ok_variant(ButtonVariant::Danger)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::DeleteSticker {
						guild_id: guild,
						sticker_id: id,
					});
					true
				})
		});
	}

	fn card(&self, sticker: &Sticker, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = sticker.id;
		let name = sticker.name.clone();
		let editing = sticker.clone();
		let preview = sticker_url(sticker).map(|url| {
			cover(
				&url,
				theme.picker_sticker,
				theme.picker_sticker,
				ObjectFit::Contain,
				cx,
			)
		});
		let detail: SharedString = sticker.tags.clone().unwrap_or_default().into();
		div()
			.flex()
			.flex_col()
			.items_center()
			.gap(theme.space_8)
			.w(theme.manage_grid_cell + theme.space_32)
			.p(theme.space_12)
			.rounded(theme.radius_md)
			.bg(theme.background_secondary)
			.child(
				div()
					.size(theme.picker_sticker)
					.flex()
					.items_center()
					.justify_center()
					.children(preview),
			)
			.child(
				div()
					.w_full()
					.truncate()
					.text_center()
					.text_size(theme.font_md)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(sticker.name.clone()),
			)
			.when(!detail.is_empty(), |card| {
				card.child(
					div()
						.w_full()
						.truncate()
						.text_center()
						.text_size(theme.font_tiny)
						.text_color(theme.text_muted)
						.child(detail),
				)
			})
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_4)
					.child(
						IconButton::new(("sticker-edit", id.get()), IconKind::Edit)
							.tooltip(cx.t("stickers-edit"))
							.on_click(cx.listener(move |this, _, window, cx| {
								sticker_edit::open(window, cx, this.data.clone(), this.guild, editing.clone())
							})),
					)
					.child(
						IconButton::new(("sticker-delete", id.get()), IconKind::Trash)
							.color(theme.danger)
							.tooltip(cx.t("menu-delete"))
							.on_click(cx.listener(move |this, _, window, cx| {
								this.confirm_delete(id, name.clone(), window, cx)
							})),
					),
			)
			.into_any_element()
	}

	fn form(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let picked = self.upload.as_ref()?;
		let enabled = self.name.read(cx).value().trim().chars().count() >= NAME_MIN;
		Some(
			div()
				.flex()
				.gap(theme.space_16)
				.mb(theme.space_16)
				.p(theme.space_16)
				.rounded(theme.radius_md)
				.bg(theme.background_secondary)
				.child(
					div().flex_none().size(theme.picker_sticker).child(
						img(picked.preview.clone())
							.size_full()
							.object_fit(ObjectFit::Contain),
					),
				)
				.child(
					div()
						.flex()
						.flex_col()
						.flex_1()
						.gap(theme.space_12)
						.child(field(cx.t("stickers-name"), theme, Input::new(&self.name)))
						.child(field(
							cx.t("stickers-description"),
							theme,
							Input::new(&self.description),
						))
						.child(field(cx.t("stickers-tags"), theme, Input::new(&self.tags)))
						.child(
							div()
								.flex()
								.justify_end()
								.gap(theme.space_8)
								.child(
									Button::new("sticker-cancel")
										.ghost()
										.label(cx.t("action-cancel"))
										.on_click(cx.listener(|this, _, _, cx| {
											this.upload = None;
											cx.notify();
										})),
								)
								.child(
									Button::new("sticker-submit")
										.primary()
										.disabled(!enabled)
										.label(cx.t("stickers-upload-submit"))
										.on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
								),
						),
				)
				.into_any_element(),
		)
	}
}

impl Render for StickersPage {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		if let Some(name) = self.pending_name.take() {
			self.name
				.update(cx, |state, cx| state.set_value(name, window, cx));
		}
		let stickers = self.stickers(cx);
		let count = stickers.len();
		let cards: Vec<AnyElement> = stickers
			.iter()
			.map(|sticker| self.card(sticker, &theme, cx))
			.collect();
		let empty = cards.is_empty();
		let form = self.form(&theme, cx);
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-stickers"), &theme))
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.justify_between()
					.pb(theme.space_16)
					.child(kit::count_caption(
						cx.t_args(
							"stickers-slots",
							&[("used", ArgValue::from(count)), ("total", ArgValue::from(SLOTS))],
						),
						&theme,
					))
					.child(
						Button::new("sticker-upload")
							.primary()
							.icon(gpui_component::Icon::default().path(IconKind::Upload.path()))
							.label(cx.t("stickers-upload"))
							.on_click(cx.listener(|_, _, _, cx| {
								pick_image(cx, None, |this: &mut Self, picked, cx| {
									this.pending_name = Some(stem(&picked.name));
									this.upload = Some(picked);
									cx.notify();
								})
							})),
					),
			)
			.children(form)
			.child(
				div()
					.flex()
					.flex_wrap()
					.content_start()
					.gap(theme.space_12)
					.pb(theme.space_32)
					.children(cards)
					.when(empty, |grid| {
						grid.child(kit::empty(IconKind::Sticker, cx.t("stickers-empty"), &theme))
					})
					.overflow_y_scrollbar()
					.id("stickers-grid")
					.flex_1()
					.min_h_0(),
			)
	}
}
