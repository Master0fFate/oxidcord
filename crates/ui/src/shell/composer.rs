use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytes::Bytes;
use gpui::{
	AnyElement, AppContext as _, ClickEvent, Context, Entity, EventEmitter, ExternalPaths, Focusable as _,
	FontWeight, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _, Pixels, Point, Render,
	SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Task, Window,
	div, img, prelude::FluentBuilder as _,
};
use gpui_component::input::{Escape, InputEvent, Textarea, TextareaState};
use media::SizeHint;
use protocol::{ChannelType, Snowflake};
use rest::UploadFile;
use session::Command;

use super::pickers::PickerTab;
use super::plugins::{command_list, composer_toolbar, parse_command};
use crate::components::{Avatar, IconButton};
use crate::data::Data;
use crate::emoji;
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::images::Images;
use crate::lookup::avatar_for;
use crate::theme::{Theme, rem, with_alpha};

const DRAFT_DELAY: Duration = Duration::from_millis(500);
const TYPING_INTERVAL: Duration = Duration::from_secs(5);
const MAX_SUGGESTIONS: usize = 8;
const MIN_EMOJI_QUERY: usize = 2;
const MAX_FILES: usize = 10;
const AVATAR_PIXELS: u32 = 48;
const EMOJI_PIXELS: u32 = 48;

pub enum ComposerEvent {
	OpenPicker { tab: PickerTab, at: Point<Pixels> },
	Sent,
}

struct ReplyTo {
	message: Snowflake,
	name: SharedString,
}

struct Attached {
	name: String,
	content_type: &'static str,
	bytes: Bytes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
	Mention,
	Channel,
	Emoji,
	Command,
}

enum Visual {
	Avatar(String),
	Glyph(String),
	Image(String),
	Channel(IconKind),
}

struct Suggestion {
	label: String,
	detail: Option<String>,
	insert: String,
	visual: Visual,
}

struct Autocomplete {
	start: usize,
	cursor: usize,
	items: Vec<Suggestion>,
	selected: usize,
}

pub struct Composer {
	data: Entity<Data>,
	input: Entity<TextareaState>,
	guild: Option<Snowflake>,
	channel: Option<Snowflake>,
	name: SharedString,
	direct: bool,
	reply: Option<ReplyTo>,
	files: Vec<Attached>,
	typing: Vec<SharedString>,
	autocomplete: Option<Autocomplete>,
	draft_task: Option<Task<()>>,
	last_typing: Option<Instant>,
	awaiting_draft: bool,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<ComposerEvent> for Composer {}

pub(in crate::shell) fn content_type(name: &str) -> &'static str {
	let extension = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
	match extension.as_str() {
		"png" => "image/png",
		"jpg" | "jpeg" => "image/jpeg",
		"gif" => "image/gif",
		"webp" => "image/webp",
		"mp4" => "video/mp4",
		"webm" => "video/webm",
		"mp3" => "audio/mpeg",
		"ogg" => "audio/ogg",
		"wav" => "audio/wav",
		"txt" | "log" | "md" => "text/plain",
		"json" => "application/json",
		"pdf" => "application/pdf",
		"zip" => "application/zip",
		_ => "application/octet-stream",
	}
}

fn trigger_of(character: char) -> Option<Trigger> {
	match character {
		'@' => Some(Trigger::Mention),
		'#' => Some(Trigger::Channel),
		':' => Some(Trigger::Emoji),
		'/' => Some(Trigger::Command),
		_ => None,
	}
}

impl Composer {
	pub fn new(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let weak = cx.weak_entity();
		let input = cx.new(|cx| {
			TextareaState::new(window, cx)
				.auto_grow(1, 10)
				.submit_on_enter(true)
		});
		let change = cx.subscribe_in(
			&input,
			window,
			|this, _, event: &InputEvent, window, cx| match event {
				InputEvent::Change => this.on_change(cx),
				InputEvent::PressEnter {
					secondary: false,
					shift: false,
				} => this.send(window, cx),
				_ => {}
			},
		);
		let keys = cx.intercept_keystrokes(move |event, window, cx| {
			let keystroke = &event.keystroke;
			if keystroke.modifiers.modified() {
				return;
			}
			let consumed = weak
				.update(cx, |this, cx| this.on_key(keystroke.key.as_str(), window, cx))
				.unwrap_or(false);
			if consumed {
				cx.stop_propagation();
			}
		});
		Self {
			data,
			input,
			guild: None,
			channel: None,
			name: SharedString::default(),
			direct: false,
			reply: None,
			files: Vec::new(),
			typing: Vec::new(),
			autocomplete: None,
			draft_task: None,
			last_typing: None,
			awaiting_draft: false,
			_subscriptions: vec![change, keys],
		}
	}

	pub fn show(
		&mut self,
		guild: Option<Snowflake>,
		channel: Option<Snowflake>,
		name: SharedString,
		direct: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.channel == channel {
			self.name = name;
			self.direct = direct;
			cx.notify();
			return;
		}
		self.flush_draft(cx);
		self.guild = guild;
		self.channel = channel;
		self.name = name;
		self.direct = direct;
		self.reply = None;
		self.files.clear();
		self.autocomplete = None;
		self.last_typing = None;
		let draft = channel
			.and_then(|channel| self.data.read(cx).state().draft(channel).map(str::to_owned))
			.unwrap_or_default();
		let placeholder = self.placeholder(cx);
		self.awaiting_draft = draft.is_empty();
		self.input.update(cx, |state, cx| {
			state.set_placeholder(placeholder, window, cx);
			state.set_value(draft, window, cx);
		});
		cx.notify();
	}

	pub fn on_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		if !self.awaiting_draft || !self.input.read(cx).value().is_empty() {
			return;
		}
		let draft = self.data.read(cx).state().draft(channel).map(str::to_owned);
		if let Some(draft) = draft {
			self.awaiting_draft = false;
			self.input
				.update(cx, |state, cx| state.set_value(draft, window, cx));
		}
	}

	pub fn set_typing(&mut self, names: Vec<SharedString>, cx: &mut Context<Self>) {
		if self.typing != names {
			self.typing = names;
			cx.notify();
		}
	}

	pub fn set_reply(
		&mut self,
		message: Snowflake,
		name: SharedString,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.reply = Some(ReplyTo { message, name });
		self.input.update(cx, |state, cx| state.focus(window, cx));
		cx.notify();
	}

	pub fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
		self.input.update(cx, |state, cx| {
			state.insert(text.to_owned(), window, cx);
			state.focus(window, cx);
		});
	}

	fn placeholder(&self, cx: &Context<Self>) -> SharedString {
		let key = if self.direct {
			"composer-placeholder-direct"
		} else {
			"composer-placeholder"
		};
		cx.t_args(key, &[("name", ArgValue::from(self.name.to_string()))])
	}

	fn flush_draft(&mut self, cx: &mut Context<Self>) {
		if self.draft_task.take().is_some() {
			self.save_draft(cx);
		}
	}

	fn save_draft(&mut self, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let content = self.input.read(cx).value().to_string();
		self.data.read(cx).send(Command::SetDraft {
			channel_id: channel,
			content,
		});
	}

	fn on_change(&mut self, cx: &mut Context<Self>) {
		self.awaiting_draft = false;
		let text = self.input.read(cx).value();
		if !text.is_empty()
			&& let Some(channel) = self.channel
			&& self
				.last_typing
				.is_none_or(|sent| sent.elapsed() > TYPING_INTERVAL)
		{
			self.last_typing = Some(Instant::now());
			self.data.read(cx).send(Command::Typing { channel_id: channel });
		}
		self.draft_task = Some(cx.spawn(async move |this, cx| {
			cx.background_executor().timer(DRAFT_DELAY).await;
			this.update(cx, |this, cx| {
				this.draft_task = None;
				this.save_draft(cx);
			})
			.ok();
		}));
		self.update_autocomplete(&text, cx);
	}

	fn update_autocomplete(&mut self, text: &str, cx: &mut Context<Self>) {
		let mut cursor = self.input.read(cx).cursor().min(text.len());
		while !text.is_char_boundary(cursor) {
			cursor -= 1;
		}
		let before = &text[..cursor];
		let start = before.rfind(char::is_whitespace).map_or(0, |index| {
			index + before[index..].chars().next().map_or(1, char::len_utf8)
		});
		let token = &before[start..];
		let trigger = token.chars().next().and_then(trigger_of);
		let query = &token[token.chars().next().map_or(0, char::len_utf8)..];
		let usable = match trigger {
			Some(Trigger::Emoji) => query.chars().count() >= MIN_EMOJI_QUERY && !query.contains(':'),
			Some(Trigger::Command) => start == 0,
			Some(_) => true,
			None => false,
		};
		let items = match trigger {
			Some(trigger) if usable => self.suggestions(trigger, &query.to_lowercase(), cx),
			_ => Vec::new(),
		};
		self.autocomplete = (!items.is_empty()).then_some(Autocomplete {
			start,
			cursor,
			items,
			selected: 0,
		});
		cx.notify();
	}

	fn suggestions(&self, trigger: Trigger, query: &str, cx: &Context<Self>) -> Vec<Suggestion> {
		let data = self.data.read(cx);
		let state = data.state();
		let rank = |name: &str| -> Option<u8> {
			let name = name.to_lowercase();
			if name.starts_with(query) {
				Some(0)
			} else {
				name.contains(query).then_some(1)
			}
		};
		let mut found: Vec<(u8, Suggestion)> = Vec::new();
		match trigger {
			Trigger::Mention => {
				let mut seen: Vec<Snowflake> = Vec::new();
				let mut candidates: Vec<Snowflake> = Vec::new();
				if let Some(guild) = self.guild {
					for row in state.member_rows(guild) {
						if let state::MemberListRow::Member(member) = row {
							candidates.push(member.user_id);
						}
					}
				}
				if let Some(channel) = self.channel {
					if let Some(cache) = state.messages(channel) {
						candidates.extend(
							cache
								.ordered
								.iter()
								.rev()
								.take(200)
								.map(|message| message.author.id),
						);
					}
					if let Some(channel) = state.channel(channel) {
						candidates.extend(channel.recipient_ids.iter().copied());
					}
				}
				for id in candidates {
					if seen.contains(&id) || Some(id) == state.current_user_id() {
						continue;
					}
					seen.push(id);
					let display = state.display_name(self.guild, id);
					let username = state.user(id).map(|user| user.username.clone());
					let hit = rank(&display).or_else(|| username.as_deref().and_then(rank));
					if let Some(hit) = hit {
						found.push((
							hit,
							Suggestion {
								label: display,
								detail: username,
								insert: format!("<@{id}> "),
								visual: Visual::Avatar(avatar_for(&state, id, AVATAR_PIXELS)),
							},
						));
					}
				}
			}
			Trigger::Channel => {
				if let Some(guild) = self.guild {
					for node in state.guild_channel_tree(guild) {
						let category = node.category.as_ref().map(|category| category.name.clone());
						for channel in node.children.iter().filter(|channel| !channel.locked) {
							if matches!(channel.kind, ChannelType::GuildForum | ChannelType::GuildMedia) {
								continue;
							}
							if let Some(hit) = rank(&channel.name) {
								found.push((
									hit,
									Suggestion {
										label: channel.name.clone(),
										detail: category.clone(),
										insert: format!("<#{}> ", channel.id),
										visual: Visual::Channel(crate::icons::channel_icon(channel.kind)),
									},
								));
							}
						}
					}
				}
			}
			Trigger::Command => {
				for hint in command_list(data) {
					if let Some(hit) = rank(&hint.name) {
						found.push((
							hit,
							Suggestion {
								label: format!("/{}", hint.name),
								detail: Some(hint.description),
								insert: format!("/{} ", hint.name),
								visual: Visual::Channel(IconKind::Plugins),
							},
						));
					}
				}
			}
			Trigger::Emoji => {
				if let Some(guild) = self.guild.and_then(|id| state.guild(id)) {
					for custom in &guild.emojis {
						let (Some(id), Some(name)) = (custom.id, custom.name.as_deref()) else {
							continue;
						};
						if let Some(hit) = rank(name) {
							let animated = custom.animated == Some(true);
							let prefix = if animated { "a" } else { "" };
							found.push((
								hit,
								Suggestion {
									label: format!(":{name}:"),
									detail: None,
									insert: format!("<{prefix}:{name}:{id}> "),
									visual: Visual::Image(media::cdn::emoji_url(
										id.get(),
										animated,
										EMOJI_PIXELS,
									)),
								},
							));
						}
					}
				}
				for (name, glyph) in emoji::all() {
					if let Some(hit) = rank(name) {
						found.push((
							hit,
							Suggestion {
								label: format!(":{name}:"),
								detail: None,
								insert: format!("{glyph} "),
								visual: Visual::Glyph(glyph.to_owned()),
							},
						));
					}
				}
			}
		}
		found.sort_by_key(|(hit, _)| *hit);
		found
			.into_iter()
			.take(MAX_SUGGESTIONS)
			.map(|(_, suggestion)| suggestion)
			.collect()
	}

	fn on_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
		if !self.input.focus_handle(cx).is_focused(window) {
			return false;
		}
		if let Some(popup) = &mut self.autocomplete {
			let count = popup.items.len();
			match key {
				"down" => popup.selected = (popup.selected + 1) % count,
				"up" => popup.selected = (popup.selected + count - 1) % count,
				"tab" | "enter" => {
					let index = popup.selected;
					self.apply(index, window, cx);
					return true;
				}
				"escape" => self.autocomplete = None,
				_ => return false,
			}
			cx.notify();
			return true;
		}
		if key == "escape" && self.reply.is_some() {
			self.reply = None;
			cx.notify();
			return true;
		}
		false
	}

	fn apply(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
		let Some(popup) = self.autocomplete.take() else {
			return;
		};
		let Some(item) = popup.items.get(index) else {
			return;
		};
		let insert = item.insert.clone();
		let range = popup.start..popup.cursor;
		self.input.update(cx, |state, cx| {
			state.set_selected_range(range, cx);
			state.replace(insert, window, cx);
			state.focus(window, cx);
		});
		cx.notify();
	}

	fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let text = self.input.read(cx).value().trim().to_owned();
		if text.is_empty() && self.files.is_empty() {
			return;
		}
		let files: Vec<UploadFile> = std::mem::take(&mut self.files)
			.into_iter()
			.map(|file| UploadFile {
				name: file.name,
				bytes: file.bytes,
				content_type: file.content_type.to_owned(),
			})
			.collect();
		let reply_to = self.reply.take().map(|reply| reply.message);
		let data = self.data.read(cx);
		match parse_command(data, &text).filter(|_| files.is_empty()) {
			Some((name, args)) => data.send(Command::RunPluginCommand {
				name,
				args,
				channel_id: channel,
			}),
			None => data.send(Command::SendMessage {
				channel_id: channel,
				content: text,
				reply_to,
				files,
				by_plugin: None,
			}),
		}
		data.send(Command::SetDraft {
			channel_id: channel,
			content: String::new(),
		});
		self.draft_task = None;
		self.autocomplete = None;
		self.last_typing = None;
		self.awaiting_draft = false;
		self.input.update(cx, |state, cx| state.clean(window, cx));
		cx.emit(ComposerEvent::Sent);
		cx.notify();
	}

	fn add_file(&mut self, name: String, bytes: Vec<u8>, cx: &mut Context<Self>) {
		if self.files.len() >= MAX_FILES {
			return;
		}
		self.files.push(Attached {
			content_type: content_type(&name),
			name,
			bytes: Bytes::from(bytes),
		});
		cx.notify();
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
					.update(cx, |this, cx| this.add_file(name, bytes, cx))
					.is_err()
				{
					return;
				}
			}
		})
		.detach();
	}

	fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
		cx.spawn(async move |this, cx| {
			for path in paths {
				let name = path
					.file_name()
					.map(|name| name.to_string_lossy().into_owned())
					.unwrap_or_default();
				let read = cx
					.background_executor()
					.spawn(async move { std::fs::read(path) })
					.await;
				match read {
					Ok(bytes) => {
						if this
							.update(cx, |this, cx| this.add_file(name, bytes, cx))
							.is_err()
						{
							return;
						}
					}
					Err(error) => tracing::warn!(%error, "could not read the dropped file"),
				}
			}
		})
		.detach();
	}

	fn paste_image(&mut self, cx: &mut Context<Self>) -> bool {
		let image = match platform::clipboard::read_image() {
			Ok(Some(image)) => image,
			Ok(None) => return false,
			Err(error) => {
				tracing::warn!(%error, "could not read the clipboard image");
				return false;
			}
		};
		let (Ok(width), Ok(height)) = (u32::try_from(image.width), u32::try_from(image.height)) else {
			return false;
		};
		let Some(buffer) = image::RgbaImage::from_raw(width, height, image.bytes) else {
			return false;
		};
		let mut encoded = Cursor::new(Vec::new());
		if let Err(error) = buffer.write_to(&mut encoded, image::ImageFormat::Png) {
			tracing::warn!(%error, "could not encode the pasted image");
			return false;
		}
		let name = format!("pasted-{}.png", chrono::Utc::now().timestamp());
		self.add_file(name, encoded.into_inner(), cx);
		true
	}

	fn typing_line(&self, cx: &Context<Self>) -> Option<SharedString> {
		match self.typing.as_slice() {
			[] => None,
			[one] => Some(cx.t_args(
				"composer-typing-one",
				&[("name", ArgValue::from(one.to_string()))],
			)),
			[first, second] => Some(cx.t_args(
				"composer-typing-two",
				&[
					("first", ArgValue::from(first.to_string())),
					("second", ArgValue::from(second.to_string())),
				],
			)),
			_ => Some(cx.t("composer-typing-several")),
		}
	}

	fn suggestion_row(
		&self,
		index: usize,
		item: &Suggestion,
		selected: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let visual: AnyElement = match &item.visual {
			Visual::Avatar(url) => Avatar::new(item.label.clone())
				.size(rem(24.))
				.url(url.clone())
				.into_any_element(),
			Visual::Glyph(glyph) => div().text_size(rem(20.)).child(glyph.clone()).into_any_element(),
			Visual::Image(url) => {
				let image = cx.global::<Images>().get(url, SizeHint::square(EMOJI_PIXELS));
				div()
					.size(rem(24.))
					.children(image.map(|image| img(image).size_full().object_fit(ObjectFit::Contain)))
					.into_any_element()
			}
			Visual::Channel(icon) => Icon::new(*icon)
				.size(rem(20.))
				.color(theme.text_muted)
				.into_any_element(),
		};
		div()
			.id(("suggestion", index))
			.flex()
			.items_center()
			.gap(theme.space_8)
			.h(rem(36.))
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.when(selected, |row| row.bg(theme.modifier_selected))
			.hover(|style| style.bg(theme.modifier_hover))
			.on_click(cx.listener(move |this, _, window, cx| {
				if let Some(popup) = &mut this.autocomplete {
					popup.selected = index;
				}
				this.apply(index, window, cx);
			}))
			.child(visual)
			.child(
				div()
					.min_w_0()
					.truncate()
					.text_size(theme.font_md)
					.font_weight(FontWeight::MEDIUM)
					.child(item.label.clone()),
			)
			.children(item.detail.clone().map(|detail| {
				div()
					.min_w_0()
					.truncate()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(detail)
			}))
			.into_any_element()
	}

	fn popup(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let popup = self.autocomplete.as_ref()?;
		let rows: Vec<AnyElement> = popup
			.items
			.iter()
			.enumerate()
			.map(|(index, item)| self.suggestion_row(index, item, index == popup.selected, theme, cx))
			.collect();
		Some(
			div()
				.absolute()
				.bottom_full()
				.left_0()
				.right_0()
				.mb(theme.space_8)
				.flex()
				.flex_col()
				.p(theme.space_8)
				.rounded(theme.radius_md)
				.border_1()
				.border_color(theme.divider)
				.bg(theme.background_floating)
				.text_color(theme.text_normal)
				.children(rows)
				.into_any_element(),
		)
	}

	fn file_chips(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		if self.files.is_empty() {
			return None;
		}
		Some(
			div()
				.flex()
				.flex_wrap()
				.gap(theme.space_8)
				.p(theme.space_8)
				.children(self.files.iter().enumerate().map(|(index, file)| {
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
								.size(rem(20.))
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
							IconButton::new(("file-remove", index), IconKind::Close)
								.icon_size(rem(16.))
								.hit_size(rem(24.))
								.tooltip(cx.t("composer-remove-file"))
								.on_click(cx.listener(move |this, _, _, cx| {
									if index < this.files.len() {
										this.files.remove(index);
										cx.notify();
									}
								})),
						)
				}))
				.into_any_element(),
		)
	}

	fn reply_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let reply = self.reply.as_ref()?;
		Some(
			div()
				.flex()
				.items_center()
				.justify_between()
				.h(rem(32.))
				.px(theme.space_12)
				.rounded_t(theme.radius_md)
				.bg(theme.background_secondary)
				.text_size(theme.font_sm)
				.text_color(theme.text_muted)
				.child(cx.t_args(
					"composer-replying",
					&[("name", ArgValue::from(reply.name.to_string()))],
				))
				.child(
					IconButton::new("composer-cancel-reply", IconKind::Close)
						.icon_size(rem(16.))
						.hit_size(rem(24.))
						.tooltip(cx.t("composer-cancel-reply"))
						.on_click(cx.listener(|this, _, _, cx| {
							this.reply = None;
							cx.notify();
						})),
				)
				.into_any_element(),
		)
	}
}

impl Render for Composer {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let typing = self.typing_line(cx);
		let weak = cx.weak_entity();
		let has_reply = self.reply.is_some();
		let chips = self.file_chips(&theme, cx);
		let popup = self.popup(&theme, cx);
		let reply_bar = self.reply_bar(&theme, cx);
		div()
			.relative()
			.flex()
			.flex_col()
			.flex_none()
			.px(theme.space_16)
			.bg(theme.background_primary)
			.on_action(cx.listener(|this, _: &Escape, _, cx| {
				if this.reply.take().is_some() {
					cx.notify();
				} else {
					cx.propagate();
				}
			}))
			.on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
				this.add_paths(paths.paths().to_vec(), cx);
			}))
			.drag_over::<ExternalPaths>(|style, _, _, cx| {
				style.bg(with_alpha(cx.global::<Theme>().brand, 0.12))
			})
			.children(popup)
			.child(
				div().flex().flex_col().child(div().children(reply_bar)).child(
					div()
						.flex()
						.flex_col()
						.rounded_b(theme.radius_md)
						.when(!has_reply, |input| input.rounded_t(theme.radius_md))
						.bg(theme.input_background)
						.children(chips)
						.child(
							div()
								.flex()
								.items_end()
								.min_h(rem(44.))
								.child(
									div()
										.flex()
										.flex_none()
										.items_center()
										.justify_center()
										.w(rem(56.))
										.h(rem(44.))
										.child(
											IconButton::new("composer-attach", IconKind::Plus)
												.icon_size(rem(24.))
												.hit_size(rem(32.))
												.tooltip(cx.t("composer-attach"))
												.on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
										),
								)
								.child(
									div()
										.flex()
										.items_center()
										.flex_1()
										.min_w_0()
										.min_h(rem(44.))
										.py(rem(2.))
										.child(
											Textarea::new(&self.input)
												.appearance(false)
												.bordered(false)
												.p_0()
												.w_full()
												.on_paste(move |_, _, cx| {
													weak.update(cx, |this, cx| this.paste_image(cx))
														.unwrap_or(false)
												}),
										),
								)
								.child(
									div()
										.flex()
										.flex_none()
										.items_center()
										.h(rem(44.))
										.pr(theme.space_8)
										.gap(rem(2.))
										.children(composer_toolbar(cx))
										.child(
											IconButton::new("composer-gif", IconKind::Gif)
												.icon_size(rem(24.))
												.hit_size(rem(32.))
												.tooltip(cx.t("composer-gif"))
												.on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
													cx.emit(ComposerEvent::OpenPicker {
														tab: PickerTab::Gif,
														at: event.position(),
													})
												})),
										)
										.child(
											IconButton::new("composer-sticker", IconKind::Sticker)
												.icon_size(rem(24.))
												.hit_size(rem(32.))
												.tooltip(cx.t("composer-sticker"))
												.on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
													cx.emit(ComposerEvent::OpenPicker {
														tab: PickerTab::Sticker,
														at: event.position(),
													})
												})),
										)
										.child(
											IconButton::new("composer-emoji", IconKind::Emoji)
												.icon_size(rem(24.))
												.hit_size(rem(32.))
												.tooltip(cx.t("composer-emoji"))
												.on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
													cx.emit(ComposerEvent::OpenPicker {
														tab: PickerTab::Emoji,
														at: event.position(),
													})
												})),
										),
								),
						),
				),
			)
			.child(
				div()
					.id("composer-typing")
					.h(rem(24.))
					.px(theme.space_4)
					.flex()
					.items_center()
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.text_normal)
					.children(typing),
			)
	}
}
