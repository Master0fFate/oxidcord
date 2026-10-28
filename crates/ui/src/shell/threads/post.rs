use std::collections::HashSet;
use std::path::PathBuf;

use bytes::Bytes;
use gpui::{
	App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, ExternalPaths, Focusable as _,
	InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
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
use crate::shell::incoming::{self, Incoming};
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

	fn can_attach(&self, cx: &App) -> bool {
		self.data.read(cx).state().can_attach(self.channel)
	}

	fn can_post(&self, cx: &App) -> bool {
		!self.title_text(cx).is_empty() && (!self.require_tag || !self.chosen.is_empty())
	}

	fn pick_files(&mut self, cx: &mut Context<Self>) {
		cx.spawn(async move |this, cx| {
			let Some(handles) = rfd::AsyncFileDialog::new().pick_files().await else {
				return;
			};
			let paths = handles.iter().map(|handle| handle.path().to_path_buf()).collect();
			this.update(cx, |this, cx| this.add_paths(paths, cx)).ok();
		})
		.detach();
	}

	fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
		self.attach_loaded(move |limit| incoming::read_paths(paths, limit), cx);
	}

	fn attach_loaded(
		&mut self,
		load: impl FnOnce(usize) -> Vec<Incoming> + Send + 'static,
		cx: &mut Context<Self>,
	) {
		let free = MAX_FILES.saturating_sub(self.files.len());
		if !self.can_attach(cx) || free == 0 {
			return;
		}
		cx.spawn(async move |this, cx| {
			let loaded = cx.background_executor().spawn(async move { load(free) }).await;
			this.update(cx, |this, cx| {
				let free = MAX_FILES.saturating_sub(this.files.len());
				this.files
					.extend(loaded.into_iter().take(free).map(|file| Attached {
						name: file.name,
						bytes: Bytes::from(file.bytes),
					}));
				cx.notify();
			})
			.ok();
		})
		.detach();
	}

	fn paste(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) -> bool {
		let paths = incoming::clipboard_paths(item);
		if !paths.is_empty() {
			self.add_paths(paths, cx);
			return true;
		}
		if !self.can_attach(cx) {
			return false;
		}
		let Some(image) = incoming::clipboard_image(item) else {
			return false;
		};
		self.attach_loaded(move |_| image.into_incoming().into_iter().collect(), cx);
		true
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if !self.can_post(cx) {
			return;
		}
		let files = std::mem::take(&mut self.files)
			.into_iter()
			.map(|file| UploadFile {
				content_type: rest::content_type_for(&file.name).to_owned(),
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
		let can_attach = self.can_attach(cx);
		let weak_cancel = cx.weak_entity();
		let weak_submit = cx.weak_entity();
		let weak_paste = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
				this.add_paths(paths.paths().to_vec(), cx);
			}))
			.drag_over::<ExternalPaths>(|style, _, _, cx| {
				style.bg(with_alpha(cx.global::<Theme>().brand, 0.12))
			})
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
					.child(
						Textarea::new(&self.body)
							.appearance(false)
							.bordered(false)
							.p_0()
							.on_paste(move |item, _, cx| {
								weak_paste
									.update(cx, |this, cx| this.paste(item, cx))
									.unwrap_or(false)
							}),
					),
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
					.when(can_attach, |row| {
						row.child(
							Button::new("post-attach")
								.small()
								.outline()
								.icon(gpui_component::Icon::default().path(IconKind::Attach.path()))
								.label(cx.t("composer-attach"))
								.on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
						)
					}),
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
