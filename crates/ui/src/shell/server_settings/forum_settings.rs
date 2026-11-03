use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
	Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Disableable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::switch::Switch;
use protocol::{Channel, ChannelFlags, ChannelType, DefaultReaction, ForumLayout, ForumSortOrder, ForumTag, Snowflake};
use session::{ForumEdit, ForumTagEdit};

use super::channel::SLOWMODES;
use crate::components::{IconButton, PickHandler, ToggleHandler, field, select, switch_row};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::shell::emoji_picker::{EmojiChoice, EmojiPicker, EmojiPickerEvent};
use crate::theme::Theme;

const TAG_LIMIT: usize = 20;
const TAG_NAME_LIMIT: usize = 20;

const LAYOUTS: [(&str, ForumLayout); 3] = [
	("forum-layout-not-set", ForumLayout::NotSet),
	("forum-layout-list", ForumLayout::ListView),
	("forum-layout-gallery", ForumLayout::GalleryView),
];
const SORTS: [(&str, ForumSortOrder); 2] = [
	("forum-sort-activity", ForumSortOrder::LatestActivity),
	("forum-sort-creation", ForumSortOrder::CreationDate),
];

#[derive(Clone, PartialEq)]
struct TagDraft {
	id: Option<Snowflake>,
	name: String,
	moderated: bool,
	emoji: Option<EmojiChoice>,
}

#[derive(Clone, PartialEq)]
struct Settings {
	tags: Vec<TagDraft>,
	require_tag: bool,
	reaction: Option<EmojiChoice>,
	layout: usize,
	sort: usize,
	slowmode: usize,
}

struct TagRow {
	id: Option<Snowflake>,
	name: Entity<InputState>,
	moderated: bool,
	emoji: Option<EmojiChoice>,
	_subscription: Subscription,
}

#[derive(Clone, Copy)]
enum Target {
	Tag(usize),
	Reaction,
}

pub struct ForumSettings {
	guild: Snowflake,
	data: Entity<Data>,
	kind: ChannelType,
	tags: Vec<TagRow>,
	require_tag: bool,
	reaction: Option<EmojiChoice>,
	layout: usize,
	sort: usize,
	slowmode: usize,
	original: Settings,
	picker: Entity<EmojiPicker>,
	picking: Option<Target>,
	_subscription: Subscription,
}

fn emoji_choice(
	id: Option<Snowflake>,
	name: Option<&str>,
	guild_emojis: &[protocol::Emoji],
) -> Option<EmojiChoice> {
	match (id, name) {
		(Some(id), name) => {
			let found = guild_emojis.iter().find(|emoji| emoji.id == Some(id));
			Some(EmojiChoice::Custom {
				name: found
					.and_then(|emoji| emoji.name.clone())
					.or_else(|| name.map(str::to_owned))
					.unwrap_or_default(),
				id: id.get(),
				animated: found.and_then(|emoji| emoji.animated) == Some(true),
			})
		}
		(None, Some(name)) if !name.is_empty() => Some(EmojiChoice::Unicode(name.to_owned())),
		_ => None,
	}
}

fn emoji_parts(choice: &EmojiChoice) -> (Option<Snowflake>, Option<String>) {
	match choice {
		EmojiChoice::Unicode(glyph) => (None, Some(glyph.clone())),
		EmojiChoice::Custom { id, .. } => (Some(Snowflake(*id)), None),
	}
}

fn slowmode_index(seconds: Option<i64>) -> usize {
	let seconds = u32::try_from(seconds.unwrap_or(0)).unwrap_or(0);
	SLOWMODES
		.iter()
		.position(|(_, limit)| *limit >= seconds)
		.unwrap_or(0)
}

impl ForumSettings {
	pub fn new(
		data: Entity<Data>,
		guild: Snowflake,
		channel: &Channel,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let guild_emojis = data
			.read(cx)
			.state()
			.guild(guild)
			.map(|found| found.emojis.clone())
			.unwrap_or_default();
		let tags = channel
			.available_tags
			.iter()
			.map(|tag| Self::row(tag, &guild_emojis, window, cx))
			.collect();
		let reaction = channel.default_reaction_emoji.as_ref().and_then(|reaction| {
			emoji_choice(reaction.emoji_id, reaction.emoji_name.as_deref(), &guild_emojis)
		});
		let picker = cx.new(|cx| EmojiPicker::new(window, cx));
		let subscription = cx.subscribe(&picker, |this, _, event: &EmojiPickerEvent, cx| {
			let EmojiPickerEvent::Pick(choice) = event;
			this.pick_emoji(choice.clone(), cx);
		});
		let mut settings = Self {
			guild,
			data,
			kind: channel.kind,
			tags,
			require_tag: channel.flags.contains(ChannelFlags::REQUIRE_TAG),
			reaction,
			layout: LAYOUTS
				.iter()
				.position(|(_, layout)| Some(*layout) == channel.default_forum_layout)
				.unwrap_or(0),
			sort: usize::from(channel.default_sort_order == Some(ForumSortOrder::CreationDate)),
			slowmode: slowmode_index(channel.default_thread_rate_limit_per_user),
			original: Settings {
				tags: Vec::new(),
				require_tag: false,
				reaction: None,
				layout: 0,
				sort: 0,
				slowmode: 0,
			},
			picker,
			picking: None,
			_subscription: subscription,
		};
		settings.original = settings.current(cx);
		settings
	}

	fn row(
		tag: &ForumTag,
		guild_emojis: &[protocol::Emoji],
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> TagRow {
		let value = tag.name.clone();
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(value, window, cx);
			state
		});
		TagRow {
			id: Some(tag.id),
			moderated: tag.moderated,
			emoji: emoji_choice(tag.emoji_id, tag.emoji_name.as_deref(), guild_emojis),
			_subscription: Self::watch(&name, window, cx),
			name,
		}
	}

	fn watch(name: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) -> Subscription {
		cx.subscribe_in(name, window, |_, _, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		})
	}

	fn current(&self, cx: &App) -> Settings {
		Settings {
			tags: self
				.tags
				.iter()
				.map(|row| TagDraft {
					id: row.id,
					name: row
						.name
						.read(cx)
						.value()
						.trim()
						.chars()
						.take(TAG_NAME_LIMIT)
						.collect(),
					moderated: row.moderated,
					emoji: row.emoji.clone(),
				})
				.collect(),
			require_tag: self.require_tag,
			reaction: self.reaction.clone(),
			layout: self.layout,
			sort: self.sort,
			slowmode: self.slowmode,
		}
	}

	pub fn can_save(&self, cx: &App) -> bool {
		let now = self.current(cx);
		now != self.original && now.tags.iter().all(|tag| !tag.name.is_empty())
	}

	pub fn edit(&self, cx: &App) -> ForumEdit {
		let now = self.current(cx);
		let was = &self.original;
		ForumEdit {
			available_tags: (now.tags != was.tags).then(|| {
				now.tags
					.iter()
					.map(|tag| {
						let (emoji_id, emoji_name) = tag.emoji.as_ref().map(emoji_parts).unwrap_or_default();
						ForumTagEdit {
							id: tag.id,
							name: tag.name.clone(),
							moderated: tag.moderated,
							emoji_id,
							emoji_name,
						}
					})
					.collect()
			}),
			default_reaction: (now.reaction != was.reaction).then(|| {
				now.reaction.as_ref().map(|choice| {
					let (emoji_id, emoji_name) = emoji_parts(choice);
					DefaultReaction { emoji_id, emoji_name }
				})
			}),
			default_sort_order: (now.sort != was.sort).then(|| SORTS[now.sort].1),
			default_layout: (self.kind == ChannelType::GuildForum && now.layout != was.layout)
				.then(|| LAYOUTS[now.layout].1),
			default_thread_slowmode: (now.slowmode != was.slowmode).then(|| SLOWMODES[now.slowmode].1),
			require_tag: (now.require_tag != was.require_tag).then_some(now.require_tag),
		}
	}

	fn add_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.tags.len() >= TAG_LIMIT {
			return;
		}
		let placeholder = cx.t("forum-tag-name-placeholder");
		let name = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscription = Self::watch(&name, window, cx);
		self.tags.push(TagRow {
			id: None,
			name,
			moderated: false,
			emoji: None,
			_subscription: subscription,
		});
		cx.notify();
	}

	fn open_picker(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
		self.picking = Some(target);
		let custom: Vec<(String, u64, bool)> = self
			.data
			.read(cx)
			.state()
			.guild(self.guild)
			.map(|guild| {
				guild
					.emojis
					.iter()
					.filter(|emoji| emoji.available != Some(false))
					.filter_map(|emoji| {
						Some((emoji.name.clone()?, emoji.id?.get(), emoji.animated == Some(true)))
					})
					.collect()
			})
			.unwrap_or_default();
		self.picker
			.update(cx, |picker, cx| picker.open(custom, window, cx));
		cx.notify();
	}

	fn pick_emoji(&mut self, choice: EmojiChoice, cx: &mut Context<Self>) {
		match self.picking.take() {
			Some(Target::Tag(index)) => {
				if let Some(row) = self.tags.get_mut(index) {
					row.emoji = Some(choice);
				}
			}
			Some(Target::Reaction) => self.reaction = Some(choice),
			None => {}
		}
		cx.notify();
	}

	fn emoji_button(
		&self,
		id: impl Into<gpui::ElementId>,
		emoji: Option<&EmojiChoice>,
		target: Target,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let chosen = emoji.map(EmojiChoice::status_emoji);
		div()
			.id(id)
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.field_height)
			.rounded(theme.radius_sm)
			.bg(theme.input_background)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_selected))
			.on_click(cx.listener(move |this, _, window, cx| this.open_picker(target, window, cx)))
			.child(match &chosen {
				Some(emoji) => emoji.element(theme.icon_lg, cx),
				None => Icon::new(IconKind::Emoji)
					.size(theme.icon_lg)
					.color(theme.interactive_normal)
					.into_any_element(),
			})
			.into_any_element()
	}

	fn tag_row(&self, index: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let row = &self.tags[index];
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.child(self.emoji_button(
				("forum-tag-emoji", index),
				row.emoji.as_ref(),
				Target::Tag(index),
				theme,
				cx,
			))
			.child(div().flex_1().min_w_0().child(Input::new(&row.name)))
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_8)
					.text_size(theme.font_sm)
					.text_color(theme.header_secondary)
					.child(cx.t("forum-tag-moderated"))
					.child(
						Switch::new(("forum-tag-moderated", index))
							.checked(row.moderated)
							.on_click(cx.listener(move |this, value: &bool, _, cx| {
								if let Some(row) = this.tags.get_mut(index) {
									row.moderated = *value;
								}
								cx.notify();
							})),
					),
			)
			.child(
				IconButton::new(("forum-tag-remove", index), IconKind::Close)
					.icon_size(theme.glyph_sm)
					.hit_size(theme.portrait_sm)
					.tooltip(cx.t("forum-tag-remove"))
					.on_click(cx.listener(move |this, _, _, cx| {
						if index < this.tags.len() {
							if matches!(this.picking, Some(Target::Tag(at)) if at >= index) {
								this.picking = None;
							}
							this.tags.remove(index);
							cx.notify();
						}
					})),
			)
			.into_any_element()
	}

	fn tags_section(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let rows: Vec<AnyElement> = (0..self.tags.len())
			.map(|index| self.tag_row(index, theme, cx))
			.collect();
		let full = self.tags.len() >= TAG_LIMIT;
		field(
			cx.t("forum-tags"),
			theme,
			div().flex().flex_col().gap(theme.space_8).children(rows).child(
				div().flex().child(
					Button::new("forum-tag-add")
						.outline()
						.disabled(full)
						.icon(gpui_component::Icon::default().path(IconKind::Plus.path()))
						.label(cx.t("forum-tag-add"))
						.on_click(cx.listener(|this, _, window, cx| this.add_tag(window, cx))),
				),
			),
		)
	}

	fn reaction_section(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		field(
			cx.t("forum-default-reaction"),
			theme,
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.child(self.emoji_button(
					"forum-reaction-emoji",
					self.reaction.as_ref(),
					Target::Reaction,
					theme,
					cx,
				))
				.when(self.reaction.is_some(), |row| {
					row.child(
						Button::new("forum-reaction-clear")
							.ghost()
							.label(cx.t("forum-reaction-clear"))
							.on_click(cx.listener(|this, _, _, cx| {
								this.reaction = None;
								cx.notify();
							})),
					)
				}),
		)
	}

	fn choice(
		&self,
		key: &'static str,
		keys: Vec<&'static str>,
		selected: usize,
		apply: fn(&mut Self, usize),
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let labels: Vec<SharedString> = keys.into_iter().map(|key| cx.t(key)).collect();
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				apply(this, index);
				cx.notify();
			})
			.ok();
		});
		field(
			cx.t(key),
			theme,
			select(key, labels[selected].clone(), &labels, selected, pick),
		)
	}
}

impl Render for ForumSettings {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let weak = cx.weak_entity();
		let require: ToggleHandler = Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| {
				this.require_tag = value;
				cx.notify();
			})
			.ok();
		});
		let slowmodes: Vec<&'static str> = SLOWMODES.iter().map(|(key, _)| *key).collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_20)
			.pb(theme.space_8)
			.child(self.tags_section(&theme, cx))
			.child(switch_row(
				"forum-require-tag",
				cx.t("forum-require-tag"),
				Some(cx.t("forum-require-tag-hint")),
				self.require_tag,
				&theme,
				require,
			))
			.child(self.reaction_section(&theme, cx))
			.when(self.kind == ChannelType::GuildForum, |form| {
				form.child(self.choice(
					"forum-layout",
					LAYOUTS.iter().map(|(key, _)| *key).collect(),
					self.layout,
					|this, index| this.layout = index,
					&theme,
					cx,
				))
			})
			.child(self.choice(
				"forum-sort",
				SORTS.iter().map(|(key, _)| *key).collect(),
				self.sort,
				|this, index| this.sort = index,
				&theme,
				cx,
			))
			.child(self.choice(
				"forum-message-slowmode",
				slowmodes,
				self.slowmode,
				|this, index| this.slowmode = index,
				&theme,
				cx,
			))
			.when(self.picking.is_some(), |form| form.child(self.picker.clone()))
			.overflow_y_scrollbar()
			.id("forum-settings")
			.h(theme.popover_height)
	}
}
