use std::collections::HashSet;

use bytes::Bytes;
use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, InteractiveElement as _,
	IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
	Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use protocol::{ChannelFlags, ForumTag, Snowflake};
use rest::UploadFile;
use session::Command;

use crate::components::{IconButton, dialog_footer, field};
use crate::data::Data;
use crate::format;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::shell::community::modal::{self, Close};
use crate::shell::composer::content_type;
use crate::theme::{Theme, rem, with_alpha};

const TITLE_LIMIT: usize = 100;
const MAX_FILES: usize = 10;

struct Attached {
	name: String,
	bytes: Bytes,
}

pub struct NewPost {
	data: Entity<Data>,
	channel: Snowflake,
	tags: Vec<ForumTag>,
	require_tag: bool,
	chosen: HashSet<Snowflake>,
	title: Entity<InputState>,
	body: Entity<TextareaState>,
	files: Vec<Attached>,
	_subscription: gpui::Subscription,
}

impl EventEmitter<Close> for NewPost {}

pub fn open(window: &mut Window, cx: &mut App, data: Entity<Data>, channel: Snowflake) {
	let view = cx.new(|cx| NewPost::new(data, channel, window, cx));
	let focus = view.read(cx).title.read(cx).focus_handle(cx);
	let title = cx.t("forum-post-title");
	let width = cx.global::<Theme>().modal_width_md;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl NewPost {
	fn new(data: Entity<Data>, channel: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let (tags, require_tag) = {
			let data = data.read(cx);
			let state = data.state();
			state
				.channel(channel)
				.map(|forum| {
					(
						forum.available_tags.to_vec(),
						forum.flags.contains(ChannelFlags::REQUIRE_TAG),
					)
				})
				.unwrap_or_default()
		};
		let title_placeholder = cx.t("forum-post-name-placeholder");
		let body_placeholder = cx.t("forum-post-body-placeholder");
		let title = cx.new(|cx| InputState::new(window, cx).placeholder(title_placeholder));
		let body = cx.new(|cx| {
			TextareaState::new(window, cx)
				.auto_grow(4, 10)
				.placeholder(body_placeholder)
		});
		let subscription = cx.subscribe_in(&title, window, |_, _, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		});
		Self {
			data,
			channel,
			tags,
			require_tag,
			chosen: HashSet::new(),
			title,
			body,
			files: Vec::new(),
			_subscription: subscription,
		}
	}

	fn title_text(&self, cx: &App) -> String {
		self.title
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(TITLE_LIMIT)
			.collect()
	}

	fn can_post(&self, cx: &App) -> bool {
		!self.title_text(cx).is_empty() && (!self.require_tag || !self.chosen.is_empty())
	}

	fn pick_files(&mut self, cx: &mut Context<Self>) {
		cx.spawn(async move |this, cx| {
			let Some(handles) = rfd::AsyncFileDialog::new().pick_files().await else {
				return;
			};
			for handle in handles {
				let name = handle.file_name();
				let bytes = handle.read().await;
				if this
					.update(cx, |this, cx| {
						if this.files.len() < MAX_FILES {
							this.files.push(Attached {
								name,
								bytes: Bytes::from(bytes),
							});
							cx.notify();
						}
					})
					.is_err()
				{
					return;
				}
			}
		})
		.detach();
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if !self.can_post(cx) {
			return;
		}
		let files = std::mem::take(&mut self.files)
			.into_iter()
			.map(|file| UploadFile {
				content_type: content_type(&file.name).to_owned(),
				name: file.name,
				bytes: file.bytes,
				description: None,
			})
			.collect();
		let mut tags: Vec<Snowflake> = self.chosen.iter().copied().collect();
		tags.sort_unstable();
		self.data.read(cx).send(Command::CreateForumPost {
			channel_id: self.channel,
			name: self.title_text(cx),
			content: self.body.read(cx).value().trim().to_owned(),
			tags,
			files,
		});
		cx.emit(Close);
	}
}

impl Render for NewPost {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let tag_chips: Vec<_> = self
			.tags
			.iter()
			.map(|tag| {
				let id = tag.id;
				let active = self.chosen.contains(&id);
				let label: SharedString = match tag.emoji_name.as_deref().filter(|emoji| !emoji.is_empty()) {
					Some(emoji) => format!("{emoji} {}", tag.name).into(),
					None => tag.name.clone().into(),
				};
				div()
					.id(("post-tag", id.get()))
					.flex()
					.items_center()
					.h(rem(28.))
					.px(theme.space_12)
					.rounded_full()
					.cursor_pointer()
					.border_1()
					.border_color(if active { theme.brand } else { theme.divider })
					.bg(if active {
						with_alpha(theme.brand, 0.2)
					} else {
						theme.background_secondary
					})
					.text_size(theme.font_sm)
					.text_color(if active {
						theme.interactive_active
					} else {
						theme.interactive_normal
					})
					.on_click(cx.listener(move |this, _, _, cx| {
						if !this.chosen.remove(&id) {
							this.chosen.insert(id);
						}
						cx.notify();
					}))
					.child(label)
			})
			.collect();
		let chips: Vec<_> = self
			.files
			.iter()
			.enumerate()
			.map(|(index, file)| {
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.max_w(rem(240.))
					.h(rem(36.))
					.pl(theme.space_8)
					.pr(theme.space_4)
					.rounded(theme.radius_sm)
					.bg(theme.background_secondary)
					.child(
						Icon::new(IconKind::File)
							.size(theme.glyph_md)
							.color(theme.interactive_normal),
					)
					.child(
						div()
							.flex()
							.flex_col()
							.min_w_0()
							.child(
								div()
									.truncate()
									.text_size(theme.font_sm)
									.text_color(theme.text_normal)
									.child(file.name.clone()),
							)
							.child(
								div()
									.text_size(theme.font_tiny)
									.text_color(theme.text_muted)
									.child(format::file_size(file.bytes.len() as u64)),
							),
					)
					.child(
						IconButton::new(("post-file-remove", index), IconKind::Close)
							.icon_size(theme.glyph_sm)
							.hit_size(theme.portrait_sm)
							.tooltip(cx.t("composer-remove-file"))
							.on_click(cx.listener(move |this, _, _, cx| {
								if index < this.files.len() {
									this.files.remove(index);
									cx.notify();
								}
							})),
					)
			})
			.collect();
		let enabled = self.can_post(cx);
		let weak_cancel = cx.weak_entity();
		let weak_submit = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(cx.t("forum-post-name"), &theme, Input::new(&self.title)))
			.child(field(
				cx.t("forum-post-body"),
				&theme,
				div()
					.rounded(theme.radius_md)
					.bg(theme.input_background)
					.px(theme.space_12)
					.py(theme.space_8)
					.child(Textarea::new(&self.body).appearance(false).bordered(false).p_0()),
			))
			.child(field(
				cx.t("forum-post-files"),
				&theme,
				div()
					.flex()
					.flex_wrap()
					.items_center()
					.gap(theme.space_8)
					.children(chips)
					.child(
						Button::new("post-attach")
							.small()
							.outline()
							.icon(gpui_component::Icon::default().path(IconKind::Attach.path()))
							.label(cx.t("composer-attach"))
							.on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
					),
			))
			.when(!tag_chips.is_empty(), |form| {
				form.child(field(
					cx.t(if self.require_tag {
						"forum-post-tags-required"
					} else {
						"forum-post-tags"
					}),
					&theme,
					div().flex().flex_wrap().gap(theme.space_8).children(tag_chips),
				))
			})
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("forum-post-submit"),
				false,
				enabled,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_submit.update(cx, |this, cx| this.submit(cx)).ok();
				},
			))
	}
}
