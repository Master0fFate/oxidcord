use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ObjectFit, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
	StyledImage as _, Subscription, Window, div, img, prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{Disableable as _, WindowExt as _};
use media::SizeHint;
use protocol::{Emoji, Snowflake};
use session::Command;

use super::kit;
use crate::components::{Avatar, IconButton};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::images::{Images, frame_id};
use crate::lookup::avatar_for;
use crate::shell::community::files::{Picked, pick_image};
use crate::theme::Theme;

const EMOJI_SIDE: u32 = 128;
const SLOTS: usize = 50;
const NAME_MIN: usize = 2;
const NAME_MAX: usize = 32;
const THUMB_PIXELS: u32 = 96;

pub struct EmojiPage {
	data: Entity<Data>,
	guild: Snowflake,
	renaming: Option<Snowflake>,
	rename: Entity<InputState>,
	upload: Option<Picked>,
	upload_name: Entity<InputState>,
	pending_name: Option<String>,
	_subscriptions: Vec<Subscription>,
}

fn sanitize(text: &str) -> String {
	text.chars()
		.filter(|character| character.is_ascii_alphanumeric() || *character == '_')
		.take(NAME_MAX)
		.collect()
}

fn stem(name: &str) -> String {
	sanitize(name.rsplit_once('.').map_or(name, |(stem, _)| stem))
}

impl EmojiPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let rename = cx.new(|cx| InputState::new(window, cx));
		let upload_name = cx.new(|cx| InputState::new(window, cx));
		let subscriptions = vec![
			cx.subscribe_in(
				&rename,
				window,
				|this, input, event: &InputEvent, _, cx| match event {
					InputEvent::PressEnter { .. } => {
						let name = sanitize(&input.read(cx).value());
						if let Some(id) = this.renaming.take()
							&& name.len() >= NAME_MIN
						{
							this.commit_rename(id, name, cx);
						}
						cx.notify();
					}
					InputEvent::Blur => {
						this.renaming = None;
						cx.notify();
					}
					_ => {}
				},
			),
			cx.subscribe_in(&upload_name, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
		];
		Self {
			data,
			guild,
			renaming: None,
			rename,
			upload: None,
			upload_name,
			pending_name: None,
			_subscriptions: subscriptions,
		}
	}

	fn emojis(&self, cx: &App) -> Vec<Emoji> {
		self.data
			.read(cx)
			.state()
			.guild(self.guild)
			.map(|guild| guild.emojis.clone())
			.unwrap_or_default()
	}

	fn commit_rename(&mut self, id: Snowflake, name: String, cx: &mut Context<Self>) {
		let roles = self
			.emojis(cx)
			.into_iter()
			.find(|emoji| emoji.id == Some(id))
			.map(|emoji| emoji.roles)
			.unwrap_or_default();
		self.data.read(cx).send(Command::EditEmoji {
			guild_id: self.guild,
			id,
			name,
			roles,
		});
	}

	fn begin_rename(&mut self, id: Snowflake, name: String, window: &mut Window, cx: &mut Context<Self>) {
		self.renaming = Some(id);
		self.rename.update(cx, |state, cx| {
			state.set_value(name, window, cx);
			state.focus(window, cx);
		});
		cx.notify();
	}

	fn confirm_delete(&mut self, id: Snowflake, name: String, window: &mut Window, cx: &mut Context<Self>) {
		let data = self.data.clone();
		let guild = self.guild;
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			alert
				.title(cx.t("emoji-delete-title"))
				.description(cx.t_args("emoji-delete-body", &[("name", ArgValue::from(name.clone()))]))
				.confirm()
				.ok_text(cx.t("menu-delete"))
				.ok_variant(ButtonVariant::Danger)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::DeleteEmoji {
						guild_id: guild,
						emoji_id: id,
					});
					true
				})
		});
	}

	fn submit_upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let name = sanitize(&self.upload_name.read(cx).value());
		if name.len() < NAME_MIN {
			return;
		}
		let Some(picked) = self.upload.take() else {
			return;
		};
		self.data.read(cx).send(Command::CreateEmoji {
			guild_id: self.guild,
			name,
			image_png: picked.bytes,
		});
		self.upload_name
			.update(cx, |state, cx| state.set_value("", window, cx));
		cx.notify();
	}

	fn card(&self, emoji: &Emoji, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let id = emoji.id?;
		let name = emoji.name.clone()?;
		let animated = emoji.animated == Some(true);
		let url = media::cdn::emoji_url(id.get(), animated, THUMB_PIXELS);
		let image = cx
			.global::<Images>()
			.get(&url, SizeHint::square(THUMB_PIXELS))
			.map(|image| {
				img(image)
					.id(frame_id(&url))
					.size_full()
					.object_fit(ObjectFit::Contain)
			});
		let (uploader, avatar) = {
			let state = self.data.read(cx).state();
			match &emoji.user {
				Some(user) => (
					state.display_name(Some(self.guild), user.id),
					avatar_for(&state, user.id, 48),
				),
				None => (String::new(), String::new()),
			}
		};
		let renaming = self.renaming == Some(id);
		let group: SharedString = format!("emoji-card-{}", id.get()).into();
		let rename_name = name.clone();
		let delete_name = name.clone();
		Some(
			div()
				.id(("emoji-card", id.get()))
				.group(group.clone())
				.relative()
				.flex()
				.items_center()
				.gap(theme.space_12)
				.p(theme.space_12)
				.rounded(theme.radius_md)
				.bg(theme.background_secondary)
				.child(
					div()
						.flex_none()
						.size(theme.portrait_lg + theme.space_8)
						.children(image),
				)
				.child(
					div()
						.flex()
						.flex_col()
						.flex_1()
						.min_w_0()
						.gap(theme.space_4)
						.child(if renaming {
							div().child(Input::new(&self.rename)).into_any_element()
						} else {
							div()
								.id(("emoji-name", id.get()))
								.truncate()
								.cursor_pointer()
								.text_size(theme.font_md)
								.font_weight(FontWeight::SEMIBOLD)
								.text_color(theme.header_primary)
								.hover(|style| style.text_color(theme.link))
								.on_click(cx.listener(move |this, _, window, cx| {
									this.begin_rename(id, rename_name.clone(), window, cx)
								}))
								.child(format!(":{name}:"))
								.into_any_element()
						})
						.when(!uploader.is_empty(), |column| {
							column.child(
								div()
									.flex()
									.items_center()
									.gap(theme.space_4)
									.text_size(theme.font_tiny)
									.text_color(theme.text_muted)
									.child(Avatar::new(uploader.clone()).size(theme.glyph_sm).url(avatar))
									.child(div().truncate().child(uploader)),
							)
						}),
				)
				.child(
					div()
						.invisible()
						.group_hover(group, |style| style.visible())
						.child(
							IconButton::new(("emoji-delete", id.get()), IconKind::Trash)
								.color(theme.danger)
								.tooltip(cx.t("menu-delete"))
								.on_click(cx.listener(move |this, _, window, cx| {
									this.confirm_delete(id, delete_name.clone(), window, cx)
								})),
						),
				)
				.into_any_element(),
		)
	}

	fn upload_form(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let picked = self.upload.as_ref()?;
		let enabled = sanitize(&self.upload_name.read(cx).value()).len() >= NAME_MIN;
		Some(
			div()
				.flex()
				.items_center()
				.gap(theme.space_12)
				.mb(theme.space_16)
				.p(theme.space_12)
				.rounded(theme.radius_md)
				.bg(theme.background_secondary)
				.child(
					div().flex_none().size(theme.portrait_lg + theme.space_8).child(
						img(picked.preview.clone())
							.size_full()
							.object_fit(ObjectFit::Contain),
					),
				)
				.child(div().flex_1().child(Input::new(&self.upload_name)))
				.child(
					Button::new("emoji-upload-cancel")
						.ghost()
						.label(cx.t("action-cancel"))
						.on_click(cx.listener(|this, _, _, cx| {
							this.upload = None;
							cx.notify();
						})),
				)
				.child(
					Button::new("emoji-upload-submit")
						.primary()
						.disabled(!enabled)
						.label(cx.t("emoji-upload-submit"))
						.on_click(cx.listener(|this, _, window, cx| this.submit_upload(window, cx))),
				)
				.into_any_element(),
		)
	}
}

impl Render for EmojiPage {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		if let Some(name) = self.pending_name.take() {
			self.upload_name
				.update(cx, |state, cx| state.set_value(name, window, cx));
		}
		let emojis = self.emojis(cx);
		let animated = emojis.iter().filter(|emoji| emoji.animated == Some(true)).count();
		let still = emojis.len() - animated;
		let cards: Vec<AnyElement> = emojis
			.iter()
			.filter_map(|emoji| self.card(emoji, &theme, cx))
			.collect();
		let empty = cards.is_empty();
		let form = self.upload_form(&theme, cx);
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-emoji"), &theme))
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.justify_between()
					.pb(theme.space_16)
					.child(
						div()
							.flex()
							.flex_col()
							.child(kit::count_caption(
								cx.t_args(
									"emoji-slots-static",
									&[("used", ArgValue::from(still)), ("total", ArgValue::from(SLOTS))],
								),
								&theme,
							))
							.child(kit::count_caption(
								cx.t_args(
									"emoji-slots-animated",
									&[
										("used", ArgValue::from(animated)),
										("total", ArgValue::from(SLOTS)),
									],
								),
								&theme,
							)),
					)
					.child(
						Button::new("emoji-upload")
							.primary()
							.icon(gpui_component::Icon::default().path(IconKind::Upload.path()))
							.label(cx.t("emoji-upload"))
							.on_click(cx.listener(|_, _, _, cx| {
								pick_image(cx, Some(EMOJI_SIDE), |this: &mut Self, picked, cx| {
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
					.children(
						cards
							.into_iter()
							.map(|card| div().w(theme.manage_grid_cell * 3.).child(card)),
					)
					.when(empty, |grid| {
						grid.child(kit::empty(IconKind::Emoji, cx.t("emoji-empty"), &theme))
					})
					.overflow_y_scrollbar()
					.id("emoji-grid")
					.flex_1()
					.min_h_0(),
			)
	}
}
