use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytes::Bytes;
use gpui::{
	Animation, AnimationExt as _, AnyElement, AppContext as _, ClickEvent, ClipboardItem, Context, Div,
	Entity, EventEmitter, ExternalPaths, Focusable as _, FontWeight, InteractiveElement as _, IntoElement,
	ObjectFit, ParentElement as _, Pixels, Point, Render, SharedString, StatefulInteractiveElement as _,
	Styled as _, StyledImage as _, Subscription, Task, Window, div, img, prelude::FluentBuilder as _,
	pulsating_between, px,
};
use gpui_component::input::{Escape, InputEvent, Textarea, TextareaState};
use gpui_component::menu::ContextMenuExt as _;
use media::SizeHint;
use plugin_host::ViewContext;
use plugin_host::api::model::{EmojiInfo, EmojiRef};
use protocol::expressions::unicode::search_unicode;
use protocol::{ChannelType, Snowflake};
use rest::UploadFile;
use session::{Command, ReplyTarget};

use super::attachments::{self, CardAction, Edit, Pending};
use super::incoming::{self, Incoming};
use super::inline_emoji::{self, EmojiTokens};
use super::pickers::{PickerTab, SkinTone, toned};
use super::plugins::{
	self, MenuTarget, PluginMenu as _, command_list, composer_toolbar, has_plugin_items, parse_command,
};
use crate::components::{Avatar, ContextMenuBuilder, Divider, IconButton, unicode_emoji};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::images::{Images, frame_id};
use crate::lookup::avatar_for;
use crate::theme::{Theme, rem, with_alpha};

const DRAFT_DELAY: Duration = Duration::from_millis(500);
const AUTOCOMPLETE_RESORT_WINDOW: Duration = Duration::from_millis(300);
const TYPING_INTERVAL: Duration = Duration::from_secs(5);
const MAX_SUGGESTIONS: usize = 8;
const MAX_CANDIDATES: usize = 200;
const TYPING_PULSE: Duration = Duration::from_millis(1200);
const MIN_EMOJI_QUERY: usize = 2;
const MAX_FILES: usize = 10;
const MAX_STICKERS: usize = 3;
const AVATAR_PIXELS: u32 = 48;
const EMOJI_PIXELS: u32 = 48;

pub enum ComposerEvent {
	OpenPicker { tab: PickerTab, at: Point<Pixels> },
	Sent,
}

#[derive(Clone, Copy, PartialEq, Default)]
struct SendAccess {
	send: bool,
	attach: bool,
}

struct ReplyTo {
	message: Snowflake,
	name: SharedString,
	mention: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
	Mention,
	Channel,
	Emoji,
	Command,
}

#[derive(Clone)]
enum Visual {
	Avatar(String),
	Glyph(String),
	Image(String),
	Channel(IconKind),
}

#[derive(Clone)]
struct Suggestion {
	label: String,
	detail: Option<String>,
	insert: String,
	visual: Visual,
	emoji: Option<EmojiInfo>,
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
	access: SendAccess,
	reply: Option<ReplyTo>,
	files: Vec<Pending>,
	stickers: Vec<Snowflake>,
	next_file: u64,
	typing: Vec<SharedString>,
	autocomplete: Option<Autocomplete>,
	emoji_candidates: Vec<Suggestion>,
	emoji_generation: u64,
	emoji_requested: Instant,
	emoji_tokens: EmojiTokens,
	draft_task: Option<Task<()>>,
	last_typing: Option<Instant>,
	awaiting_draft: bool,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<ComposerEvent> for Composer {}

pub(in crate::shell) fn accept_files(element: Div, composer: Entity<Composer>) -> Div {
	element
		.on_drop(move |paths: &ExternalPaths, _, cx| {
			composer.update(cx, |composer, cx| composer.add_paths(paths.paths().to_vec(), cx));
		})
		.drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(with_alpha(cx.global::<Theme>().brand, 0.12)))
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
				InputEvent::Change => this.on_change(window, cx),
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
		let changes = cx.subscribe_in(&data, window, |this, _, event: &DataEvent, window, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.guild_list
					|| this
						.guild
						.is_some_and(|guild| changes.channel_lists.contains(guild)))
			{
				this.refresh_access(window, cx);
			}
		});
		Self {
			data,
			input,
			guild: None,
			channel: None,
			name: SharedString::default(),
			direct: false,
			access: SendAccess::default(),
			reply: None,
			files: Vec::new(),
			stickers: Vec::new(),
			next_file: 0,
			typing: Vec::new(),
			autocomplete: None,
			emoji_candidates: Vec::new(),
			emoji_generation: 0,
			emoji_requested: Instant::now(),
			emoji_tokens: EmojiTokens::default(),
			draft_task: None,
			last_typing: None,
			awaiting_draft: false,
			_subscriptions: vec![change, keys, changes],
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
		self.access = self.read_access(cx);
		self.reply = None;
		self.files.clear();
		self.stickers.clear();
		self.autocomplete = None;
		self.last_typing = None;
		let draft = channel
			.and_then(|channel| self.data.read(cx).state().draft(channel).map(str::to_owned))
			.unwrap_or_default();
		let placeholder = self.placeholder(cx);
		self.awaiting_draft = draft.is_empty();
		self.input.update(cx, |state, cx| {
			state.set_placeholder(placeholder, window, cx);
			state.set_value(inline_emoji::content(draft), window, cx);
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
			self.input.update(cx, |state, cx| {
				state.set_value(inline_emoji::content(draft), window, cx)
			});
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
		mention: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.reply = Some(ReplyTo {
			message,
			name,
			mention,
		});
		self.input.update(cx, |state, cx| state.focus(window, cx));
		cx.notify();
	}

	pub fn set_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
		self.input.update(cx, |state, cx| {
			state.set_value(inline_emoji::content(text), window, cx);
			state.focus(window, cx);
		});
	}

	pub fn add_sticker(&mut self, sticker: Snowflake, cx: &mut Context<Self>) {
		if self.access.send && self.stickers.len() < MAX_STICKERS && !self.stickers.contains(&sticker) {
			self.stickers.push(sticker);
			cx.notify();
		}
	}

	pub fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
		self.input.update(cx, |state, cx| {
			inline_emoji::insert(state, text, window, cx);
			state.focus(window, cx);
		});
	}

	fn read_access(&self, cx: &Context<Self>) -> SendAccess {
		let Some(channel) = self.channel else {
			return SendAccess::default();
		};
		if self.guild.is_none() {
			return SendAccess {
				send: true,
				attach: true,
			};
		}
		let data = self.data.read(cx);
		let state = data.state();
		SendAccess {
			send: state.can_send(channel),
			attach: state.can_attach(channel),
		}
	}

	fn refresh_access(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let access = self.read_access(cx);
		if access == self.access {
			return;
		}
		self.access = access;
		if !access.attach {
			self.files.clear();
		}
		if !access.send {
			self.reply = None;
			self.stickers.clear();
			self.autocomplete = None;
		}
		let placeholder = self.placeholder(cx);
		self.input
			.update(cx, |state, cx| state.set_placeholder(placeholder, window, cx));
		cx.notify();
	}

	fn placeholder(&self, cx: &Context<Self>) -> SharedString {
		if !self.access.send {
			return cx.t("composer-no-permission");
		}
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

	fn on_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.awaiting_draft = false;
		self.emoji_tokens.sync(&self.input, window, cx);
		let text = self.input.read(cx).value();
		if !text.is_empty()
			&& let Some(channel) = self.channel
			&& !self.data.read(cx).session().plugins().hides_typing()
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
		self.update_autocomplete(&text, window, cx);
	}

	fn update_autocomplete(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
		self.emoji_candidates.clear();
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
		if trigger == Some(Trigger::Emoji)
			&& let Some(item) = self.shortcode(query, cx)
		{
			self.autocomplete = Some(Autocomplete {
				start,
				cursor,
				items: vec![item],
				selected: 0,
			});
			self.apply(0, window, cx);
			return;
		}
		let items = match trigger {
			Some(Trigger::Emoji) if usable && plugins::autocomplete_declared(cx) => {
				self.plugin_emoji(&query.to_lowercase(), cx)
			}
			Some(trigger) if usable => self.suggestions(trigger, &query.to_lowercase(), MAX_SUGGESTIONS, cx),
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

	fn plugin_emoji(&mut self, query: &str, cx: &mut Context<Self>) -> Vec<Suggestion> {
		let all = self.suggestions(Trigger::Emoji, query, MAX_CANDIDATES, cx);
		let candidates: Vec<EmojiInfo> = all.iter().filter_map(|item| item.emoji.clone()).collect();
		self.emoji_generation += 1;
		self.emoji_requested = Instant::now();
		self.data.read(cx).send(Command::PluginAutocompleteEmoji {
			query: query.to_owned(),
			generation: self.emoji_generation,
			candidates,
		});
		let shown = all.iter().take(MAX_SUGGESTIONS).cloned().collect();
		self.emoji_candidates = all;
		shown
	}

	pub fn on_autocomplete(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		let untouched = self
			.autocomplete
			.as_ref()
			.is_some_and(|popup| popup.selected == 0);
		if self.emoji_candidates.is_empty()
			|| !untouched
			|| self.emoji_requested.elapsed() >= AUTOCOMPLETE_RESORT_WINDOW
		{
			return;
		}
		let order = self
			.data
			.read(cx)
			.state()
			.autocomplete_order(self.emoji_generation)
			.map(<[u32]>::to_vec);
		let Some(order) = order else {
			return;
		};
		let mut taken = vec![false; self.emoji_candidates.len()];
		let mut sorted: Vec<Suggestion> = Vec::with_capacity(MAX_SUGGESTIONS);
		for index in order.into_iter().map(|index| index as usize) {
			if let Some(item) = self.emoji_candidates.get(index)
				&& !std::mem::replace(&mut taken[index], true)
			{
				sorted.push(item.clone());
			}
		}
		sorted.truncate(MAX_SUGGESTIONS);
		if let Some(popup) = &mut self.autocomplete
			&& !sorted.is_empty()
		{
			popup.items = sorted;
			cx.notify();
		}
	}

	fn shortcode(&self, query: &str, cx: &Context<Self>) -> Option<Suggestion> {
		let name = query
			.strip_suffix(':')
			.filter(|name| !name.is_empty() && !name.contains(':'))?
			.to_lowercase();
		let label = format!(":{name}:");
		let mut item = self
			.suggestions(Trigger::Emoji, &name, MAX_SUGGESTIONS, cx)
			.into_iter()
			.find(|item| item.label.eq_ignore_ascii_case(&label))?;
		item.insert.truncate(item.insert.trim_end().len());
		Some(item)
	}

	fn suggestions(
		&self,
		trigger: Trigger,
		query: &str,
		limit: usize,
		cx: &Context<Self>,
	) -> Vec<Suggestion> {
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
								emoji: None,
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
										emoji: None,
									},
								));
							}
						}
					}
				}
			}
			Trigger::Command => {
				for (name, entry) in command_list(data).iter() {
					if let Some(hit) = rank(name) {
						found.push((
							hit,
							Suggestion {
								label: format!("/{name}"),
								detail: Some(entry.description.clone()),
								insert: format!("/{name} "),
								visual: Visual::Channel(IconKind::Plugins),
								emoji: None,
							},
						));
					}
				}
			}
			Trigger::Emoji => {
				let unlocked = data.session().plugins().has_emoji_pick();
				for section in state.emoji_catalog(self.guild).guilds {
					let detail = (Some(section.guild_id) != self.guild).then_some(section.name);
					for custom in section.emojis.iter().filter(|custom| custom.usable || unlocked) {
						let Some(hit) = rank(&custom.name) else {
							continue;
						};
						let prefix = if custom.animated { "a" } else { "" };
						found.push((
							hit,
							Suggestion {
								label: format!(":{}:", custom.name),
								detail: detail.clone(),
								insert: format!("<{prefix}:{}:{}> ", custom.name, custom.id),
								visual: Visual::Image(media::cdn::emoji_url(
									custom.id.get(),
									custom.animated,
									EMOJI_PIXELS,
								)),
								emoji: Some(EmojiInfo {
									emoji: EmojiRef {
										id: Some(custom.id.get()),
										name: custom.name.clone(),
										animated: custom.animated,
									},
									guild_id: Some(section.guild_id.get()),
									usable: custom.usable,
									available: true,
								}),
							},
						));
					}
				}
				let tone = cx.try_global::<SkinTone>().map_or(0, |tone| tone.0.get());
				for entry in search_unicode(query).into_iter().take(limit) {
					let glyph = if entry.skin_tone_capable {
						toned(entry.emoji, tone)
					} else {
						entry.emoji.to_owned()
					};
					found.push((
						rank(entry.shortcode).unwrap_or(2),
						Suggestion {
							label: format!(":{}:", entry.shortcode),
							detail: None,
							insert: format!("{glyph} "),
							emoji: Some(EmojiInfo {
								emoji: EmojiRef {
									id: None,
									name: glyph.clone(),
									animated: false,
								},
								guild_id: None,
								usable: true,
								available: true,
							}),
							visual: Visual::Glyph(glyph),
						},
					));
				}
			}
		}
		found.sort_by_key(|(hit, _)| *hit);
		found
			.into_iter()
			.take(limit)
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
			inline_emoji::insert(state, &insert, window, cx);
			state.focus(window, cx);
		});
		cx.notify();
	}

	fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel.filter(|_| self.access.send) else {
			return;
		};
		let text = self.input.read(cx).value().trim().to_owned();
		if text.is_empty() && self.files.is_empty() && self.stickers.is_empty() {
			return;
		}
		let stickers = std::mem::take(&mut self.stickers);
		let files: Vec<UploadFile> = std::mem::take(&mut self.files)
			.into_iter()
			.map(Pending::upload)
			.collect();
		let direct = self.direct;
		let reply_to = self.reply.take().map(|reply| ReplyTarget {
			message: reply.message,
			mention: direct || reply.mention,
		});
		let data = self.data.read(cx);
		match parse_command(data, &text).filter(|_| files.is_empty() && stickers.is_empty()) {
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
				stickers,
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
		if !self.access.attach || self.files.len() >= MAX_FILES {
			return;
		}
		let id = self.next_file;
		self.next_file += 1;
		let file = Pending::new(id, name, Bytes::from(bytes));
		if file.is_image() {
			let bytes = file.bytes.clone();
			cx.spawn(async move |this, cx| {
				let decoded = cx
					.background_executor()
					.spawn(async move { attachments::decode_preview(&bytes) })
					.await;
				this.update(cx, |this, cx| {
					if let Some(file) = this.files.iter_mut().find(|file| file.id == id) {
						file.preview = decoded.and_then(attachments::preview);
						cx.notify();
					}
				})
				.ok();
			})
			.detach();
		}
		self.files.push(file);
		cx.notify();
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

	pub(in crate::shell) fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
		self.attach_loaded(move |limit| incoming::read_paths(paths, limit), cx);
	}

	fn attach_loaded(
		&mut self,
		load: impl FnOnce(usize) -> Vec<Incoming> + Send + 'static,
		cx: &mut Context<Self>,
	) {
		let free = MAX_FILES.saturating_sub(self.files.len());
		if !self.access.attach || free == 0 {
			return;
		}
		let channel = self.channel;
		cx.spawn(async move |this, cx| {
			let loaded = cx.background_executor().spawn(async move { load(free) }).await;
			this.update(cx, |this, cx| {
				if this.channel == channel {
					for file in loaded {
						this.add_file(file.name, file.bytes, cx);
					}
				}
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
		if !self.access.attach {
			return false;
		}
		let Some(image) = incoming::clipboard_image(item) else {
			return false;
		};
		self.attach_loaded(move |_| image.into_incoming().into_iter().collect(), cx);
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
			Visual::Glyph(glyph) => unicode_emoji(glyph, rem(24.), cx),
			Visual::Image(url) => {
				let image = cx.global::<Images>().get(url, SizeHint::square(EMOJI_PIXELS));
				div()
					.size(rem(24.))
					.children(image.map(|image| {
						img(image)
							.id(frame_id(url))
							.size_full()
							.object_fit(ObjectFit::Contain)
					}))
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

	fn file_cards(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		if self.files.is_empty() {
			return None;
		}
		let cards = self.files.iter().map(|file| {
			let weak = cx.weak_entity();
			let id = file.id;
			attachments::card(
				file,
				move |action, window, cx| {
					weak.update(cx, |this, cx| this.on_card(id, action, window, cx))
						.ok();
				},
				theme,
				cx,
			)
		});
		Some(
			div()
				.id("composer-attachments")
				.flex()
				.gap(theme.space_16)
				.px(theme.space_16)
				.pt(theme.space_24)
				.pb(theme.space_12)
				.border_b_1()
				.border_color(theme.divider)
				.children(cards)
				.overflow_x_scroll()
				.into_any_element(),
		)
	}

	fn on_card(&mut self, id: u64, action: CardAction, window: &mut Window, cx: &mut Context<Self>) {
		let Some(index) = self.files.iter().position(|file| file.id == id) else {
			return;
		};
		match action {
			CardAction::Spoiler => {
				let file = &mut self.files[index];
				file.spoiler = !file.spoiler;
			}
			CardAction::Remove => {
				self.files.remove(index);
			}
			CardAction::View => {
				let file = &self.files[index];
				if let Some(preview) = file.preview.clone() {
					attachments::open_full_view(window, cx, file.name.clone(), preview);
				}
			}
			CardAction::Modify => {
				let weak = cx.weak_entity();
				attachments::open_modify(window, cx, &self.files[index], move |edit, cx| {
					weak.update(cx, |this, cx| this.apply_edit(id, edit, cx)).ok();
				});
			}
		}
		cx.notify();
	}

	fn apply_edit(&mut self, id: u64, edit: Edit, cx: &mut Context<Self>) {
		let Some(file) = self.files.iter_mut().find(|file| file.id == id) else {
			return;
		};
		file.name = edit.name;
		file.content_type = rest::content_type_for(&file.name);
		file.description = edit.description;
		file.spoiler = edit.spoiler;
		cx.notify();
		let Some(rect) = edit.crop else {
			return;
		};
		let (bytes, name) = (file.bytes.clone(), file.name.clone());
		cx.spawn(async move |this, cx| {
			let processed = cx
				.background_executor()
				.spawn(async move {
					let cropped = attachments::crop(&bytes, &name, rect)?;
					let preview = attachments::decode_preview(&cropped.bytes);
					Some((cropped, preview))
				})
				.await;
			let Some((cropped, preview)) = processed else {
				return;
			};
			this.update(cx, |this, cx| {
				if let Some(file) = this.files.iter_mut().find(|file| file.id == id) {
					file.content_type = rest::content_type_for(&cropped.name);
					file.name = cropped.name;
					file.bytes = Bytes::from(cropped.bytes);
					file.preview = preview.and_then(attachments::preview);
					cx.notify();
				}
			})
			.ok();
		})
		.detach();
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
				.border_1()
				.border_b_0()
				.border_color(theme.divider)
				.bg(theme.background_secondary)
				.text_size(theme.font_sm)
				.text_color(theme.text_muted)
				.child(cx.t_args(
					"composer-replying",
					&[("name", ArgValue::from(reply.name.to_string()))],
				))
				.child(
					div()
						.flex()
						.items_center()
						.gap(theme.space_8)
						.when(!self.direct, |actions| {
							actions
								.child(
									div()
										.id("composer-reply-ping")
										.flex()
										.items_center()
										.gap(theme.space_4)
										.cursor_pointer()
										.font_weight(FontWeight::BOLD)
										.text_color(if reply.mention {
											theme.link
										} else {
											theme.text_muted
										})
										.tooltip(crate::components::tooltip(cx.t("composer-reply-ping")))
										.on_click(cx.listener(|this, _, _, cx| {
											if let Some(reply) = &mut this.reply {
												reply.mention = !reply.mention;
												cx.notify();
											}
										}))
										.child(Icon::new(IconKind::AtSign).size(rem(16.)))
										.child(cx.t(if reply.mention {
											"composer-reply-ping-on"
										} else {
											"composer-reply-ping-off"
										})),
								)
								.child(
									Divider::vertical()
										.length(rem(16.))
										.thickness(px(1.))
										.color(theme.divider),
								)
						})
						.child(
							IconButton::new("composer-cancel-reply", IconKind::Close)
								.icon_size(rem(16.))
								.hit_size(rem(24.))
								.tooltip(cx.t("composer-cancel-reply"))
								.on_click(cx.listener(|this, _, _, cx| {
									this.reply = None;
									cx.notify();
								})),
						),
				)
				.into_any_element(),
		)
	}
}

fn typing_dots(theme: &Theme) -> impl IntoElement {
	div()
		.flex()
		.flex_none()
		.items_center()
		.gap(rem(2.))
		.children((0..3).map(|_| div().size(rem(4.)).rounded_full().bg(theme.text_normal)))
		.with_animation(
			"composer-typing-dots",
			Animation::new(TYPING_PULSE)
				.repeat()
				.with_easing(pulsating_between(0.3, 1.)),
			|dots, delta| dots.opacity(delta),
		)
}

impl Render for Composer {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let typing = self.typing_line(cx);
		let weak = cx.weak_entity();
		let has_reply = self.reply.is_some();
		let chips = self.file_cards(&theme, cx);
		let popup = self.popup(&theme, cx);
		let reply_bar = self.reply_bar(&theme, cx);
		let plugin_context = ViewContext {
			guild_id: self.guild.map(Snowflake::get),
			channel_id: self.channel.map(Snowflake::get),
			..ViewContext::default()
		};
		let above = plugins::contextual("composer-above", plugin_context, cx);
		let access = self.access;
		let animated = plugins::client_options(cx).typing_animation.unwrap_or(true);
		let selection = self.input.read(cx).selected_value().to_string();
		let menu_target = MenuTarget::composer(self.guild, self.channel, selection);
		let input_area = div()
			.id("composer-input")
			.flex()
			.items_center()
			.flex_1()
			.min_w_0()
			.min_h(rem(52.))
			.py(rem(2.))
			.child(
				Textarea::new(&self.input)
					.token(inline_emoji::render)
					.appearance(false)
					.bordered(false)
					.disabled(!access.send)
					.p_0()
					.w_full()
					.on_paste(move |item, _, cx| {
						weak.update(cx, |this, cx| this.paste(item, cx)).unwrap_or(false)
					}),
			);
		let input_area = if has_plugin_items("composer-context-menu", &menu_target, cx) {
			input_area
				.context_menu(
					ContextMenuBuilder::new()
						.plugin_items("composer-context-menu", menu_target, cx)
						.build(),
				)
				.into_any_element()
		} else {
			input_area.into_any_element()
		};
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
			.children(popup)
			.when(!above.is_empty(), |composer| {
				composer.child(
					div()
						.flex()
						.flex_col()
						.gap(theme.space_4)
						.pb(theme.space_8)
						.children(above),
				)
			})
			.child(
				div().flex().flex_col().child(div().children(reply_bar)).child(
					div()
						.flex()
						.flex_col()
						.rounded_b(theme.radius_md)
						.when(!has_reply, |input| input.rounded_t(theme.radius_md))
						.border_1()
						.border_color(theme.divider)
						.bg(theme.input_background)
						.children(chips)
						.child(
							div()
								.flex()
								.items_end()
								.min_h(rem(52.))
								.child(
									div()
										.flex()
										.flex_none()
										.items_center()
										.justify_center()
										.w(rem(56.))
										.h(rem(52.))
										.when(access.attach, |slot| {
											slot.child(
												IconButton::new("composer-attach", IconKind::CirclePlus)
													.icon_size(rem(22.))
													.hit_size(rem(32.))
													.tooltip(cx.t("composer-attach"))
													.on_click(
														cx.listener(|this, _, _, cx| this.pick_files(cx)),
													),
											)
										}),
								)
								.child(input_area)
								.when(access.send, |row| {
									row.child(
										div()
											.flex()
											.flex_none()
											.items_center()
											.h(rem(52.))
											.pr(theme.space_8)
											.gap(theme.space_4)
											.children(composer_toolbar(plugin_context, cx))
											.child(
												IconButton::new("composer-gif", IconKind::Gif)
													.icon_size(rem(22.))
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
													.icon_size(rem(22.))
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
													.icon_size(rem(22.))
													.hit_size(rem(32.))
													.tooltip(cx.t("composer-emoji"))
													.on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
														cx.emit(ComposerEvent::OpenPicker {
															tab: PickerTab::Emoji,
															at: event.position(),
														})
													})),
											)
											.child(
												IconButton::new("composer-send", IconKind::Send)
													.icon_size(rem(22.))
													.hit_size(rem(32.))
													.tooltip(cx.t("composer-send"))
													.on_click(cx.listener(|this, _, window, cx| {
														this.send(window, cx)
													})),
											),
									)
								}),
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
					.when_some(typing, |row, line| {
						row.gap(theme.space_8)
							.when(animated, |row| row.child(typing_dots(&theme)))
							.child(line)
					}),
			)
	}
}
