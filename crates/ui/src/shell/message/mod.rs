mod body;
mod components;
mod media;
mod menu;
mod player;
mod provider;
mod reaction_tip;
mod selectable;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use gpui::{
	Animation, AnimationExt as _, AnyElement, App, ClickEvent, Entity, FontWeight, Hsla,
	InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, SharedString,
	StatefulInteractiveElement as _, Styled as _, Window, div, hsla, prelude::FluentBuilder as _,
	pulsating_between,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _};
use lru::LruCache;
use markdown::Document;
use plugin_host::ViewContext;
use plugin_host::api::model::ClickTarget;
use protocol::{Message, MessageType, Snowflake};
use state::{ClientOptions, MessageRow, MessageView, RenderedMessage};

pub use self::body::snippet;
use self::body::{BodyEnv, render_body};
use self::menu::{MenuSource, capabilities, capture_pointer};
pub(in crate::shell) use self::menu::{point_link, point_media, with_attachment_menu, with_link_menu};
pub use self::reaction_tip::ReactionRequests;
use super::community;
use super::incoming;
use super::inline_emoji::{self, EmojiTokens};
use super::plugins::{
	INLINE, PatchEffects, author_badge, contextual_inline, decoration_of, message_actions, message_click,
	message_extras, patch_effects, tags,
};
use crate::components::{Avatar, IconButton, tooltip};
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::lookup::{Lookup, user_avatar};
use crate::model::role_color;
use crate::theme::{Theme, rem};

const SNIPPET_LENGTH: usize = 120;
const AVATAR_SIZE: f32 = 40.;
const SKELETON_ROWS: usize = 4;
const DELETED_OPACITY: f32 = 0.7;
const FADED_OPACITY: f32 = 0.5;
const REVEAL_INDEX: usize = usize::MAX;
const REPLY_PREVIEW_NODES: usize = 4;

#[derive(Clone, Copy)]
pub struct Prefs {
	pub inline_media: bool,
	pub link_previews: bool,
	pub show_reactions: bool,
	pub time_24h: bool,
	pub font_scale: f32,
	pub developer_mode: bool,
	pub hide_invites: bool,
}

impl Default for Prefs {
	fn default() -> Self {
		Self {
			inline_media: true,
			link_previews: true,
			show_reactions: true,
			time_24h: false,
			font_scale: 1.,
			developer_mode: false,
			hide_invites: false,
		}
	}
}

impl Prefs {
	pub fn load(cx: &App) -> Self {
		if !cx.has_global::<community::CommunityHandle>() {
			return Self::default();
		}
		let settings = community::settings(cx);
		Self {
			inline_media: settings.chat.inline_media,
			link_previews: settings.chat.link_previews,
			show_reactions: settings.chat.show_reactions,
			time_24h: settings.chat.time_24h,
			font_scale: settings.accessibility.font_scale,
			developer_mode: settings.developer.developer_mode,
			hide_invites: crate::data::streamer_mode(cx).hides_invites(),
		}
	}
}

fn clock_text(time: DateTime<Utc>, h24: bool) -> String {
	time.with_timezone(&Local)
		.format(if h24 { "%H:%M" } else { "%-I:%M %p" })
		.to_string()
}

fn stamp(time: DateTime<Utc>, h24: bool, cx: &App) -> SharedString {
	if !h24 {
		return format::message_time(time, cx);
	}
	let day = time.with_timezone(&Local).date_naive();
	let today = Local::now().date_naive();
	let clock = clock_text(time, true);
	if day == today {
		cx.t_args("time-today", &[("time", ArgValue::from(clock))])
	} else if today.pred_opt() == Some(day) {
		cx.t_args("time-yesterday", &[("time", ArgValue::from(clock))])
	} else {
		day.format("%m/%d/%Y").to_string().into()
	}
}

fn full_time(time: DateTime<Utc>, h24: bool) -> String {
	time.with_timezone(&Local)
		.format(if h24 {
			"%A, %B %-d, %Y %H:%M"
		} else {
			"%A, %B %-d, %Y %-I:%M %p"
		})
		.to_string()
}

pub type DocumentCache = RefCell<LruCache<u64, (Arc<Message>, Option<Arc<RenderedMessage>>, Rc<Document>)>>;
pub type Dispatch = Rc<dyn Fn(RowAction, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct ImageTarget {
	pub url: String,
	pub name: String,
	pub width: u32,
	pub height: u32,
}

#[derive(Clone)]
pub enum RowAction {
	OpenLink(String),
	OpenMessageLink {
		url: String,
		message_id: u64,
	},
	Download(String),
	ToggleSpoiler(u64, usize),
	OpenImage(ImageTarget),
	React {
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: String,
		add: bool,
	},
	Reply(Snowflake),
	StartEdit(Snowflake),
	Delete(Snowflake),
	Resend {
		channel_id: Snowflake,
		content: String,
		reply_to: Option<Snowflake>,
	},
	OpenEmoji {
		message_id: Snowflake,
		at: Point<Pixels>,
	},
	OpenProfile {
		user: Snowflake,
		at: Point<Pixels>,
	},
	JumpTo(Snowflake),
	CreateThread(Snowflake),
	OpenThread(Snowflake),
	Pin {
		message: Snowflake,
		pin: bool,
	},
	MarkUnread(Snowflake),
	Forward(Snowflake),
	PollVote {
		channel_id: Snowflake,
		message_id: Snowflake,
		answer_ids: Vec<i64>,
	},
	PollToggle {
		message_id: Snowflake,
		answer_id: i64,
		current: Vec<i64>,
	},
	PollEnd {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	Select(Snowflake),
	DeleteSelected,
	Report,
	UseComponent(session::ComponentUse),
	RunCommand {
		message: Snowflake,
		command: Box<protocol::ApplicationCommand>,
	},
	Save {
		message: Snowflake,
		due: Option<DateTime<Utc>>,
	},
	Publish(Snowflake),
}

#[derive(Clone, Copy, Default)]
pub struct MessageAccess {
	pub send: bool,
	pub react: bool,
	pub read_history: bool,
	pub manage_messages: bool,
	pub pin: bool,
	pub create_threads: bool,
	pub in_thread: bool,
	pub announcement: bool,
}

#[derive(Clone)]
pub struct Editing {
	pub message: Snowflake,
	pub input: Entity<TextareaState>,
	pub tokens: EmojiTokens,
}

pub struct RowEnv<'a> {
	pub theme: &'a Theme,
	pub lookup: &'a Lookup<'a>,
	pub dispatch: &'a Dispatch,
	pub revealed: &'a HashSet<(u64, usize)>,
	pub poll_picks: &'a HashMap<Snowflake, Vec<i64>>,
	pub prefs: Prefs,
	pub documents: &'a DocumentCache,
	pub me: Option<Snowflake>,
	pub compact: bool,
	pub editing: Option<&'a Editing>,
	pub welcome: &'a Welcome,
	pub access: MessageAccess,
	pub selection: &'a HashSet<Snowflake>,
	pub options: ClientOptions,
}

pub struct Welcome {
	pub name: SharedString,
	pub direct: bool,
}

fn sent_at(message: &Message) -> DateTime<Utc> {
	message.timestamp.unwrap_or_else(|| message.id.timestamp())
}

fn message_hover_background() -> Hsla {
	hsla(0., 0., 0., 0.06)
}

fn highlighted(view: &MessageView, tint: Option<Hsla>) -> bool {
	tint.is_none() && view.mentions_me
}

fn row_hover(view: &MessageView, tint: Option<Hsla>, theme: &Theme) -> Hsla {
	if highlighted(view, tint) {
		crate::theme::with_alpha(theme.warning, 0.12)
	} else {
		message_hover_background()
	}
}

fn mention_highlight(row: gpui::Stateful<gpui::Div>, theme: &Theme) -> gpui::Stateful<gpui::Div> {
	row.bg(crate::theme::with_alpha(theme.warning, 0.08)).child(
		div()
			.absolute()
			.left_0()
			.top_0()
			.bottom_0()
			.w(rem(2.))
			.bg(theme.warning),
	)
}

pub fn render_row(row: &MessageRow, env: &RowEnv, cx: &App) -> AnyElement {
	match row {
		MessageRow::HistoryStart => welcome_header(env.welcome, env.theme, cx),
		MessageRow::Loading => skeleton(env.theme),
		MessageRow::DaySeparator(day) => day_separator(format::day_label(*day).into(), env.theme),
		MessageRow::UnreadMarker => unread_marker(env.theme, cx),
		MessageRow::Message(view) => match view.message.kind {
			MessageType::Default
			| MessageType::Reply
			| MessageType::ChatInputCommand
			| MessageType::ContextMenuCommand
			| MessageType::PollResult => {
				if env.compact {
					compact_message(view, env, cx)
				} else {
					message_row(view, env, cx)
				}
			}
			_ => system_row(view, env, cx),
		},
	}
}

fn welcome_header(welcome: &Welcome, theme: &Theme, cx: &App) -> AnyElement {
	let args = [("name", ArgValue::from(welcome.name.to_string()))];
	let (title, body, icon) = if welcome.direct {
		(
			"timeline-welcome-direct-title",
			"timeline-welcome-direct-body",
			IconKind::AtSign,
		)
	} else {
		("timeline-welcome-title", "timeline-welcome-body", IconKind::Hash)
	};
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.px(theme.space_16)
		.pt(theme.space_32)
		.pb(theme.space_16)
		.child(
			div()
				.flex()
				.items_center()
				.justify_center()
				.size(rem(68.))
				.rounded_full()
				.bg(theme.interactive_muted)
				.child(Icon::new(icon).size(rem(42.)).color(theme.interactive_active)),
		)
		.child(
			div()
				.text_size(rem(32.))
				.line_height(rem(40.))
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_primary)
				.child(cx.t_args(title, &args)),
		)
		.child(
			div()
				.text_size(theme.font_md)
				.text_color(theme.text_muted)
				.child(cx.t_args(body, &args)),
		)
		.into_any_element()
}

fn skeleton(theme: &Theme) -> AnyElement {
	let line = |width: f32| {
		div()
			.h(rem(12.))
			.w(rem(width))
			.rounded_full()
			.bg(theme.interactive_muted)
	};
	div()
		.flex()
		.flex_col()
		.gap(theme.space_16)
		.px(theme.space_16)
		.py(theme.space_16)
		.children((0..SKELETON_ROWS).map(|index| {
			div()
				.flex()
				.gap(theme.space_16)
				.child(
					div()
						.flex_none()
						.size(rem(AVATAR_SIZE))
						.rounded_full()
						.bg(theme.interactive_muted),
				)
				.child(
					div()
						.flex()
						.flex_col()
						.gap(theme.space_8)
						.child(line(90. + (index as f32 * 37.) % 80.))
						.child(line(240. + (index as f32 * 61.) % 220.))
						.when(index % 2 == 0, |lines| lines.child(line(160.))),
				)
		}))
		.with_animation(
			"timeline-skeleton",
			Animation::new(Duration::from_millis(1600))
				.repeat()
				.with_easing(pulsating_between(0.35, 0.8)),
			|element, delta| element.opacity(delta),
		)
		.into_any_element()
}

fn day_separator(label: SharedString, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_8)
		.mx(theme.space_16)
		.mt(theme.space_16)
		.child(div().flex_1().h(rem(1.)).bg(theme.divider))
		.child(
			div()
				.flex_none()
				.text_size(theme.font_tiny)
				.line_height(rem(16.))
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(theme.text_muted)
				.child(label),
		)
		.child(div().flex_1().h(rem(1.)).bg(theme.divider))
		.into_any_element()
}

fn unread_marker(theme: &Theme, cx: &App) -> AnyElement {
	div()
		.relative()
		.flex()
		.items_center()
		.mx(theme.space_16)
		.my(theme.space_8)
		.h(rem(13.))
		.child(div().flex_1().h(rem(1.)).bg(theme.danger))
		.child(
			div()
				.flex_none()
				.px(rem(4.))
				.h(rem(13.))
				.rounded(theme.radius_sm)
				.bg(theme.danger)
				.text_size(rem(10.))
				.line_height(rem(13.))
				.font_weight(FontWeight::BOLD)
				.text_color(theme.text_on_brand)
				.child(cx.t("timeline-new").to_uppercase()),
		)
		.into_any_element()
}

fn system_text(view: &MessageView, env: &RowEnv, cx: &App) -> (IconKind, Hsla, SharedString) {
	let theme = env.theme;
	let message = &view.message;
	let name = || ArgValue::from(view.author_display.clone());
	let target = || {
		let user = message.mentions.first();
		ArgValue::from(user.map_or_else(String::new, |user| env.lookup.user(user.id.get())))
	};
	match message.kind {
		MessageType::UserJoin => (
			IconKind::Join,
			theme.success,
			cx.t_args("system-join", &[("name", name())]),
		),
		MessageType::ChannelPinnedMessage => (
			IconKind::Pin,
			theme.interactive_normal,
			cx.t_args("system-pin", &[("name", name())]),
		),
		MessageType::ThreadCreated => (
			IconKind::Thread,
			theme.interactive_normal,
			cx.t_args(
				"system-thread",
				&[
					("name", name()),
					("thread", ArgValue::from(message.content.clone())),
				],
			),
		),
		MessageType::PremiumGuildSubscription => (
			IconKind::Nitro,
			theme.brand,
			cx.t_args("system-boost", &[("name", name())]),
		),
		MessageType::PremiumGuildSubscriptionTier1
		| MessageType::PremiumGuildSubscriptionTier2
		| MessageType::PremiumGuildSubscriptionTier3 => {
			let tier = match message.kind {
				MessageType::PremiumGuildSubscriptionTier1 => 1usize,
				MessageType::PremiumGuildSubscriptionTier2 => 2,
				_ => 3,
			};
			(
				IconKind::Nitro,
				theme.brand,
				cx.t_args(
					"system-boost-tier",
					&[("name", name()), ("tier", ArgValue::from(tier))],
				),
			)
		}
		MessageType::RecipientAdd => (
			IconKind::Join,
			theme.success,
			cx.t_args("system-recipient-add", &[("name", name()), ("target", target())]),
		),
		MessageType::RecipientRemove => (
			IconKind::Close,
			theme.danger,
			cx.t_args(
				"system-recipient-remove",
				&[("name", name()), ("target", target())],
			),
		),
		MessageType::Call => (
			IconKind::Phone,
			theme.success,
			cx.t_args("system-call", &[("name", name())]),
		),
		MessageType::ChannelNameChange => (
			IconKind::Edit,
			theme.interactive_normal,
			cx.t_args(
				"system-rename",
				&[
					("name", name()),
					("channel", ArgValue::from(message.content.clone())),
				],
			),
		),
		MessageType::ChannelIconChange => (
			IconKind::Image,
			theme.interactive_normal,
			cx.t_args("system-icon", &[("name", name())]),
		),
		_ => (
			IconKind::Chat,
			theme.interactive_normal,
			cx.t_args("system-generic", &[("name", name())]),
		),
	}
}

fn system_row(view: &MessageView, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let (icon, color, text) = system_text(view, env, cx);
	let thread = (view.message.kind == MessageType::ThreadCreated)
		.then(|| {
			view.message.thread.as_ref().map(|thread| thread.id).or_else(|| {
				view.message
					.message_reference
					.as_ref()
					.and_then(|reference| reference.channel_id)
			})
		})
		.flatten();
	let dispatch = env.dispatch.clone();
	div()
		.id(("system", view.message.id.get()))
		.when_some(thread, |row, id| {
			row.cursor_pointer()
				.on_click(move |_, window, cx| dispatch(RowAction::OpenThread(id), window, cx))
		})
		.relative()
		.flex()
		.items_baseline()
		.gap(theme.space_8)
		.pl(rem(72.))
		.pr(rem(48.))
		.mt(rem(17.))
		.py(rem(2.))
		.hover(|style| style.bg(message_hover_background()))
		.child(
			div()
				.absolute()
				.left(rem(16.))
				.top(rem(3.))
				.w(rem(40.))
				.flex()
				.justify_end()
				.child(Icon::new(icon).size(rem(18.)).color(color)),
		)
		.child(
			div()
				.text_size(theme.font_md)
				.line_height(rem(22.))
				.text_color(theme.text_muted)
				.child(text),
		)
		.child(
			div()
				.text_size(theme.font_tiny)
				.text_color(theme.text_muted)
				.child(stamp(sent_at(&view.message), env.prefs.time_24h, cx)),
		)
		.into_any_element()
}

fn bot_tag(label: SharedString, theme: &Theme) -> impl IntoElement {
	div()
		.flex_none()
		.px(rem(5.))
		.rounded(theme.radius_sm)
		.bg(theme.brand)
		.text_size(rem(10.))
		.line_height(rem(15.))
		.font_weight(FontWeight::BOLD)
		.text_color(theme.text_on_brand)
		.child(label)
}

fn author_color(view: &MessageView, theme: &Theme) -> Hsla {
	view.author_color.map_or(theme.header_primary, role_color)
}

fn reply_header(
	view: &MessageView,
	env: &RowEnv,
	effects: Option<&PatchEffects>,
	cx: &App,
) -> Option<AnyElement> {
	let theme = env.theme;
	let message = &view.message;
	if message.is_forward() {
		return None;
	}
	message.message_reference.as_ref()?.message_id?;
	let left = |child: AnyElement, mounted: Vec<AnyElement>| {
		div()
			.relative()
			.flex()
			.items_center()
			.gap(theme.space_4)
			.h(rem(20.))
			.mb(rem(2.))
			.child(
				div()
					.absolute()
					.left(rem(-36.))
					.top(rem(10.))
					.w(rem(32.))
					.h(rem(10.))
					.border_l_2()
					.border_t_2()
					.border_color(theme.interactive_muted)
					.rounded_tl(rem(6.)),
			)
			.child(child)
			.children(mounted)
			.into_any_element()
	};
	let Some(referenced) = &message.referenced_message else {
		return Some(left(
			div()
				.text_size(theme.font_sm)
				.italic()
				.text_color(theme.text_muted)
				.child(cx.t("message-reply-deleted"))
				.into_any_element(),
			Vec::new(),
		));
	};
	let author = &referenced.author;
	let guild = message.guild_id.or(env.lookup.guild);
	let state = env.lookup.state;
	let over = state.user_override(env.lookup.guild, author.id);
	let name = over
		.and_then(|over| over.name.clone())
		.unwrap_or_else(|| env.lookup.user(author.id.get()));
	let avatar = over
		.and_then(|over| over.avatar.clone())
		.unwrap_or_else(|| user_avatar(author, 32));
	let suffix = over.and_then(|over| over.suffix.clone());
	let document = markdown::parse(&referenced.content);
	let text = if referenced.content.is_empty() {
		cx.t("message-reply-attachment").to_string()
	} else {
		snippet(&document, SNIPPET_LENGTH)
	};
	let annotation = effects
		.and_then(PatchEffects::reply_annotation)
		.map(str::to_owned);
	let mounted = contextual_inline(
		"reply-preview",
		ViewContext {
			guild_id: guild.map(Snowflake::get),
			channel_id: Some(message.channel_id.get()),
			message_id: Some(referenced.id.get()),
			user_id: Some(author.id.get()),
			attachment_id: None,
		},
		INLINE,
		REPLY_PREVIEW_NODES,
		cx,
	);
	let target = referenced.id;
	let dispatch = env.dispatch.clone();
	let replying = message.clone();
	Some(left(
		div()
			.id(("reply", message.id.get()))
			.flex()
			.items_center()
			.gap(theme.space_4)
			.min_w_0()
			.cursor_pointer()
			.on_click(move |event, window, cx| {
				if message_click(&replying, guild, event, ClickTarget::ReplyPreview, cx) {
					cx.stop_propagation();
					return;
				}
				dispatch(RowAction::JumpTo(target), window, cx)
			})
			.child(Avatar::new(name.clone()).size(rem(16.)).url(avatar))
			.child(
				div()
					.flex_none()
					.text_size(theme.font_sm)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_secondary)
					.child(format!("@{name}")),
			)
			.children(suffix.map(|suffix| {
				div()
					.flex_none()
					.text_size(theme.font_tiny)
					.text_color(theme.text_muted)
					.child(suffix)
			}))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.truncate()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(text),
			)
			.children(annotation.map(|annotation| {
				div()
					.flex_none()
					.text_size(theme.font_tiny)
					.italic()
					.text_color(theme.text_muted)
					.child(annotation)
			}))
			.into_any_element(),
		mounted,
	))
}

fn forward_block(view: &MessageView, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let message = &view.message;
	let id = message.id.get();
	let header = div()
		.id(("forward-label", id))
		.flex()
		.items_center()
		.gap(theme.space_4)
		.h(rem(20.))
		.child(
			Icon::new(IconKind::Forward)
				.size(rem(14.))
				.color(theme.text_muted),
		)
		.child(
			div()
				.text_size(theme.font_sm)
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(theme.header_secondary)
				.child(cx.t("message-forwarded")),
		);
	let Some(source) = message.forwarded_message() else {
		return header.into_any_element();
	};
	let mut lines: Vec<AnyElement> = Vec::new();
	if !source.content.is_empty() {
		let document = markdown::parse(&source.content);
		let body_env = BodyEnv::new(theme, env.lookup, env.dispatch, id, env.revealed, env.prefs)
			.jumbo(document.is_only_emoji());
		lines.push(render_body(&document, &body_env, cx));
	}
	let body_env = BodyEnv::new(theme, env.lookup, env.dispatch, id, env.revealed, env.prefs);
	lines.extend(media::attachments(
		&source.attachments,
		&body_env,
		env.prefs.inline_media,
		cx,
	));
	if !source.flags.contains(protocol::MessageFlags::SUPPRESS_EMBEDS) {
		lines.extend(media::embeds(
			&source.embeds,
			&body_env,
			env.prefs.link_previews,
			cx,
		));
	}
	lines.extend(media::stickers(&source.sticker_items, &body_env, cx));
	if lines.is_empty() {
		return header.into_any_element();
	}
	div()
		.id(("forward", id))
		.mt(rem(1.))
		.flex()
		.flex_col()
		.child(header)
		.child(
			div()
				.flex()
				.gap(theme.space_8)
				.child(
					div()
						.flex_none()
						.w(rem(4.))
						.rounded_full()
						.bg(theme.interactive_muted),
				)
				.child(div().flex().flex_col().flex_1().min_w_0().children(lines)),
		)
		.into_any_element()
}

fn toolbar(view: &MessageView, group: SharedString, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let message = &view.message;
	let id = message.id;
	let key = id.get();
	let can = capabilities(message, env.access, env.me);
	let react = env.dispatch.clone();
	let reply = env.dispatch.clone();
	let edit = env.dispatch.clone();
	let more_menu = MenuSource::new(message, env).into_menu().build();
	let expanded = env.options.message_toolbar_expanded.unwrap_or(false);
	div()
		.when(!expanded, |bar| {
			bar.invisible().group_hover(group, |style| style.visible())
		})
		.absolute()
		.right(theme.space_16)
		.top(rem(-2.))
		.flex()
		.items_center()
		.h(rem(32.))
		.px(rem(2.))
		.rounded(theme.radius_sm)
		.border_1()
		.border_color(theme.divider)
		.bg(theme.background_primary)
		.when(can.react, |bar| {
			bar.child(
				IconButton::new(("message-react", key), IconKind::Emoji)
					.filled_hover()
					.hit_size(rem(28.))
					.icon_size(rem(18.))
					.tooltip(cx.t("message-react"))
					.on_click(move |event: &ClickEvent, window, cx| {
						react(
							RowAction::OpenEmoji {
								message_id: id,
								at: event.position(),
							},
							window,
							cx,
						)
					}),
			)
		})
		.when(can.reply, |bar| {
			bar.child(
				IconButton::new(("message-reply", key), IconKind::Reply)
					.filled_hover()
					.hit_size(rem(28.))
					.icon_size(rem(18.))
					.tooltip(cx.t("message-reply"))
					.on_click(move |_, window, cx| reply(RowAction::Reply(id), window, cx)),
			)
		})
		.when(can.edit, |bar| {
			bar.child(
				IconButton::new(("message-edit", key), IconKind::Edit)
					.filled_hover()
					.hit_size(rem(28.))
					.icon_size(rem(18.))
					.tooltip(cx.t("message-edit"))
					.on_click(move |_, window, cx| edit(RowAction::StartEdit(id), window, cx)),
			)
		})
		.child(
			Button::new(("message-more", key))
				.ghost()
				.small()
				.icon(gpui_component::Icon::default().path(IconKind::More.path()))
				.tooltip(cx.t("message-more"))
				.dropdown_menu(more_menu),
		)
		.children(message_actions(view, cx))
		.into_any_element()
}

fn document_of(env: &RowEnv, view: &MessageView) -> Rc<Document> {
	let message = &view.message;
	let mut cache = env.documents.borrow_mut();
	if let Some((cached, rendered, document)) = cache.get(&message.id.get())
		&& Arc::ptr_eq(cached, message)
		&& same_rendered(rendered, &view.rendered)
	{
		return document.clone();
	}
	let document = Rc::new(match &view.rendered {
		Some(rendered) => Document {
			nodes: rendered.ast.clone(),
		},
		None => markdown::parse(&message.content),
	});
	cache.put(
		message.id.get(),
		(message.clone(), view.rendered.clone(), document.clone()),
	);
	document
}

fn same_rendered(left: &Option<Arc<RenderedMessage>>, right: &Option<Arc<RenderedMessage>>) -> bool {
	match (left, right) {
		(Some(left), Some(right)) => Arc::ptr_eq(left, right),
		(None, None) => true,
		_ => false,
	}
}

fn veil_line(
	veil: &super::plugins::Veil,
	message: u64,
	revealed: bool,
	env: &RowEnv,
	cx: &App,
) -> AnyElement {
	let theme = env.theme;
	let key = if veil.collapsed {
		"plugin-collapsed-by"
	} else {
		"plugin-hidden-by"
	};
	let label = cx.t_args(key, &[("name", ArgValue::from(veil.plugin.to_string()))]);
	let toggle = env.dispatch.clone();
	let hover = theme.link;
	div()
		.flex()
		.items_center()
		.gap(theme.space_8)
		.text_size(theme.font_sm)
		.text_color(theme.text_muted)
		.child(
			Icon::new(IconKind::Plugins)
				.size(rem(14.))
				.color(theme.text_muted),
		)
		.child(label)
		.when(!veil.reason.is_empty(), |line| {
			line.child(format!("· {}", veil.reason))
		})
		.child(
			div()
				.id(("plugin-reveal", message))
				.cursor_pointer()
				.text_color(theme.link)
				.hover(move |style| style.underline().text_color(hover))
				.on_click(move |_, window, cx| {
					toggle(RowAction::ToggleSpoiler(message, REVEAL_INDEX), window, cx)
				})
				.child(cx.t(if revealed {
					"plugin-hide-again"
				} else {
					"plugin-show"
				})),
		)
		.into_any_element()
}

fn media_source(message: &Arc<Message>, effects: Option<&PatchEffects>) -> Option<Message> {
	let effects = effects.filter(|effects| effects.touches_media())?;
	let mut filtered = (**message).clone();
	filtered.attachments = message
		.attachments
		.iter()
		.filter(|attachment| !effects.hides_attachment(attachment.id))
		.map(|attachment| {
			let mut shown = attachment.clone();
			if let Some(url) = effects
				.attachment_override(attachment.id)
				.and_then(|over| over.url.clone())
			{
				shown.url = Some(url);
				shown.proxy_url = None;
			}
			shown
		})
		.collect::<Vec<_>>()
		.into();
	filtered.embeds = message
		.embeds
		.iter()
		.enumerate()
		.filter(|(index, _)| effects.embed_action(*index).is_none())
		.map(|(_, embed)| embed.clone())
		.collect::<Vec<_>>()
		.into();
	Some(filtered)
}

fn content_column(
	view: &MessageView,
	env: &RowEnv,
	effects: Option<&PatchEffects>,
	cx: &App,
) -> Vec<AnyElement> {
	let message = &view.message;
	let theme = env.theme;
	let mut out: Vec<AnyElement> = Vec::new();
	if let Some(veil) = effects.and_then(|effects| effects.veil(cx)) {
		let id = message.id.get();
		let revealed = env.revealed.contains(&(id, REVEAL_INDEX));
		out.push(veil_line(&veil, id, revealed, env, cx));
		if !revealed {
			out.extend(message_extras(view, None, theme, cx));
			return out;
		}
	}
	let faded = effects.is_some_and(PatchEffects::faded);
	let strike = effects.is_some_and(PatchEffects::strike);
	let body_env = BodyEnv::new(
		theme,
		env.lookup,
		env.dispatch,
		message.id.get(),
		env.revealed,
		env.prefs,
	)
	.patch(effects)
	.in_channel(message.channel_id.get());
	if let Some(editing) = env.editing.filter(|editing| editing.message == message.id) {
		out.push(editor(editing, env, cx));
	} else if message.is_forward() {
		out.push(forward_block(view, env, cx));
	} else if !message.content.is_empty() && !link_only(message, env.prefs) {
		let document = document_of(env, view);
		let trailer = if view.deleted {
			Some(cx.t("message-deleted"))
		} else {
			message.edited_timestamp.is_some().then(|| cx.t("message-edited"))
		};
		let body_env = body_env
			.jumbo(document.is_only_emoji())
			.trailer(trailer)
			.color((view.failed || view.deleted).then_some(theme.danger));
		let body = render_body(&document, &body_env, cx);
		out.push(if faded || strike {
			div()
				.when(faded, |body| body.opacity(FADED_OPACITY))
				.when(strike, |body| body.line_through())
				.child(body)
				.into_any_element()
		} else {
			body
		});
	}
	let body_env = BodyEnv::new(
		theme,
		env.lookup,
		env.dispatch,
		message.id.get(),
		env.revealed,
		env.prefs,
	)
	.patch(effects)
	.in_channel(message.channel_id.get());
	let filtered = media_source(message, effects);
	let media_message = filtered.as_ref().unwrap_or(message);
	if !message.flags.contains(protocol::MessageFlags::IS_COMPONENTS_V2) {
		out.extend(media::attachments(
			&media_message.attachments,
			&body_env,
			env.prefs.inline_media,
			cx,
		));
	}
	if !message.flags.contains(protocol::MessageFlags::SUPPRESS_EMBEDS) {
		out.extend(media::embeds(
			&media_message.embeds,
			&body_env,
			env.prefs.link_previews,
			cx,
		));
	}
	out.extend(components::render(media_message, &body_env, cx));
	if let Some(effects) = effects {
		out.extend(effects.cards(message.embeds.len(), message.id, theme));
	}
	out.extend(media::stickers(&message.sticker_items, &body_env, cx));
	out.extend(thread_chip(message, env, cx));
	if !env.prefs.hide_invites {
		out.extend(super::community::invites::cards(
			&message.content,
			env.lookup.state,
			env.theme,
			cx,
		));
	}
	if message.poll.is_some() {
		let poll = media::PollView {
			message,
			picks: env.poll_picks.get(&message.id),
			mine: Some(message.author.id) == env.me,
		};
		out.push(media::poll(&poll, &body_env, cx));
	}
	if env.prefs.show_reactions {
		out.extend(media::reactions(message, &body_env, cx));
	}
	if view.failed {
		out.push(failed_notice(view, env, cx));
	}
	out.extend(message_extras(view, effects, theme, cx));
	out
}

fn link_only(message: &Message, prefs: Prefs) -> bool {
	if !prefs.link_previews || message.flags.contains(protocol::MessageFlags::SUPPRESS_EMBEDS) {
		return false;
	}
	let content = message.content.trim();
	!content.contains(char::is_whitespace)
		&& message
			.embeds
			.iter()
			.any(|embed| embed.url.as_deref() == Some(content) && media::hides_link(embed))
}

fn thread_chip(message: &Message, env: &RowEnv, cx: &App) -> Option<AnyElement> {
	let thread = message.thread.as_ref()?;
	let theme = env.theme;
	let id = thread.id;
	let count = thread.message_count.unwrap_or(0);
	let dispatch = env.dispatch.clone();
	let activity = thread
		.last_message_id
		.map_or_else(|| thread.id.timestamp(), Snowflake::timestamp);
	Some(
		div()
			.id(("thread-chip", message.id.get()))
			.flex()
			.items_center()
			.gap(theme.space_8)
			.mt(theme.space_4)
			.px(theme.space_12)
			.py(theme.space_8)
			.max_w(theme.invite_card_width)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_secondary)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_selected))
			.on_click(move |_, window, cx| dispatch(RowAction::OpenThread(id), window, cx))
			.child(
				Icon::new(IconKind::Thread)
					.size(theme.glyph_md)
					.color(theme.interactive_normal),
			)
			.child(
				div()
					.min_w_0()
					.truncate()
					.text_size(theme.font_sm)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(thread.name.clone().unwrap_or_default()),
			)
			.child(
				div()
					.flex_none()
					.text_size(theme.font_tiny)
					.text_color(theme.text_muted)
					.child(cx.t_args("thread-chip-count", &[("count", ArgValue::from(count))])),
			)
			.child(
				div()
					.flex_none()
					.text_size(theme.font_tiny)
					.text_color(theme.text_muted)
					.child(stamp(activity, env.prefs.time_24h, cx)),
			)
			.into_any_element(),
	)
}

fn editor(editing: &Editing, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	div()
		.flex()
		.flex_col()
		.gap(theme.space_4)
		.child(
			div()
				.rounded(theme.radius_md)
				.bg(theme.input_background)
				.px(theme.space_12)
				.py(theme.space_8)
				.child(
					Textarea::new(&editing.input)
						.token(inline_emoji::render)
						.appearance(false)
						.bordered(false)
						.p_0()
						.on_paste(|item, _, _| incoming::paths_only(item)),
				),
		)
		.child(
			div()
				.text_size(theme.font_tiny)
				.text_color(theme.text_muted)
				.child(cx.t("message-edit-hint")),
		)
		.into_any_element()
}

fn failed_notice(view: &MessageView, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let message = &view.message;
	let channel_id = message.channel_id;
	let content = message.content.clone();
	let reply_to = message
		.message_reference
		.as_ref()
		.and_then(|reference| reference.message_id);
	let dispatch = env.dispatch.clone();
	div()
		.flex()
		.items_center()
		.gap(theme.space_8)
		.text_size(theme.font_sm)
		.text_color(theme.danger)
		.child(cx.t("message-failed"))
		.child(
			Button::new(("message-retry", message.id.get()))
				.small()
				.danger()
				.label(cx.t("message-retry"))
				.on_click(move |_, window, cx| {
					dispatch(
						RowAction::Resend {
							channel_id,
							content: content.clone(),
							reply_to,
						},
						window,
						cx,
					)
				}),
		)
		.into_any_element()
}

fn interactive(
	row: gpui::Stateful<gpui::Div>,
	view: &MessageView,
	group: SharedString,
	env: &RowEnv,
	cx: &App,
) -> AnyElement {
	if view.pending || view.deleted {
		return row.into_any_element();
	}
	let id = view.message.id;
	let selectable = env.access.manage_messages;
	let selecting = !env.selection.is_empty();
	let dispatch = env.dispatch.clone();
	let guild = view.message.guild_id.or(env.lookup.guild);
	let clicked = view.message.clone();
	let aux_clicked = view.message.clone();
	let row = row
		.capture_any_mouse_down(|_, window, cx| capture_pointer(window, cx))
		.when(env.selection.contains(&id), |row| {
			row.bg(crate::theme::with_alpha(env.theme.brand, 0.14))
		})
		.on_click(move |event, window, cx| {
			if message_click(&clicked, guild, event, ClickTarget::Body, cx) {
				return;
			}
			if selectable && (event.modifiers().shift || selecting) {
				dispatch(RowAction::Select(id), window, cx);
			}
		})
		.on_aux_click(move |event, _, cx| {
			if event.is_middle_click() {
				message_click(&aux_clicked, guild, event, ClickTarget::Body, cx);
			}
		});
	row.child(toolbar(view, group, env, cx))
		.context_menu(MenuSource::new(&view.message, env).into_menu().build())
		.into_any_element()
}

fn message_row(view: &MessageView, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let message = &view.message;
	let id = message.id.get();
	let group: SharedString = format!("message-{id}").into();
	let effects = patch_effects(message.id, cx);
	let reply = reply_header(view, env, effects.as_ref(), cx);
	let has_reply = reply.is_some();
	let content = content_column(view, env, effects.as_ref(), cx);
	let tip = effects
		.as_ref()
		.and_then(PatchEffects::tooltip)
		.map(str::to_owned);
	let accent = effects.as_ref().and_then(|effects| effects.accent(theme));
	let tint = effects.as_ref().and_then(|effects| effects.background(theme));
	let hover = row_hover(view, tint, theme);
	let container = div()
		.id(("message", id))
		.group(group.clone())
		.relative()
		.flex()
		.flex_col()
		.pl(rem(72.))
		.pr(rem(48.))
		.py(rem(2.))
		.hover(move |style| style.bg(hover))
		.when(view.pending, |row| row.opacity(0.5))
		.when(view.deleted, |row| row.opacity(DELETED_OPACITY))
		.when(view.failed, |row| row.text_color(theme.danger))
		.when(highlighted(view, tint), |row| mention_highlight(row, theme))
		.when_some(tint, |row, tint| row.bg(tint))
		.when_some(accent, |row, accent| row.border_l_2().border_color(accent))
		.when_some(tip, |row, tip| row.tooltip(tooltip(tip)));
	if view.grouped_with_previous {
		let row = container
			.child(
				div()
					.absolute()
					.left_0()
					.top(rem(2.))
					.w(rem(72.))
					.h(rem(22.))
					.flex()
					.items_center()
					.justify_center()
					.text_size(rem(11.))
					.text_color(theme.text_muted)
					.invisible()
					.group_hover(group.clone(), |style| style.visible())
					.child(clock_text(sent_at(message), env.prefs.time_24h)),
			)
			.children(content);
		return interactive(row, view, group, env, cx);
	}
	let name = view.author_display.clone();
	let author_id = message.author.id;
	let guild = message.guild_id.or(env.lookup.guild);
	let over = env.lookup.state.user_override(guild, author_id);
	let author_tags = over.map_or_else(Vec::new, |over| tags(over, author_id, theme, cx));
	let avatar = over
		.and_then(|over| over.avatar.clone())
		.unwrap_or_else(|| user_avatar(&message.author, 128));
	let decoration = decoration_of(over);
	let profile = env.dispatch.clone();
	let avatar_profile = env.dispatch.clone();
	let author_message = message.clone();
	let avatar_color = view.author_color.map_or(theme.brand, role_color);
	let row = container
		.mt(theme.space_16)
		.children(reply)
		.child(
			div()
				.id(("message-avatar", id))
				.absolute()
				.left(theme.space_16)
				.top(if has_reply { rem(24.) } else { rem(4.) })
				.cursor_pointer()
				.on_click(move |event, window, cx| {
					avatar_profile(
						RowAction::OpenProfile {
							user: author_id,
							at: event.position(),
						},
						window,
						cx,
					)
				})
				.child(
					Avatar::new(name.clone())
						.size(rem(AVATAR_SIZE))
						.url(avatar)
						.color(avatar_color)
						.decoration(decoration),
				),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.h(rem(22.))
				.child(
					div()
						.id(("message-author", id))
						.cursor_pointer()
						.text_size(theme.font_md)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(author_color(view, theme))
						.hover(|style| style.underline())
						.on_click(move |event, window, cx| {
							if message_click(&author_message, guild, event, ClickTarget::Author, cx) {
								cx.stop_propagation();
								return;
							}
							profile(
								RowAction::OpenProfile {
									user: author_id,
									at: event.position(),
								},
								window,
								cx,
							)
						})
						.child(name),
				)
				.children(author_tags)
				.when(message.author.bot == Some(true), |header| {
					header.child(bot_tag(cx.t("badge-bot"), theme))
				})
				.children(author_badge(view, theme))
				.child(
					div()
						.id(("message-time", id))
						.text_size(theme.font_tiny)
						.text_color(theme.text_muted)
						.tooltip(tooltip(full_time(sent_at(message), env.prefs.time_24h)))
						.child(stamp(sent_at(message), env.prefs.time_24h, cx)),
				),
		)
		.children(content);
	interactive(row, view, group, env, cx)
}

fn compact_message(view: &MessageView, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let message = &view.message;
	let id = message.id.get();
	let group: SharedString = format!("message-{id}").into();
	let effects = patch_effects(message.id, cx);
	let content = content_column(view, env, effects.as_ref(), cx);
	let accent = effects.as_ref().and_then(|effects| effects.accent(theme));
	let tint = effects.as_ref().and_then(|effects| effects.background(theme));
	let hover = row_hover(view, tint, theme);
	let tip = effects
		.as_ref()
		.and_then(PatchEffects::tooltip)
		.map(str::to_owned);
	let guild = message.guild_id.or(env.lookup.guild);
	let author_tags = env
		.lookup
		.state
		.user_override(guild, message.author.id)
		.map_or_else(Vec::new, |over| tags(over, message.author.id, theme, cx));
	let row = div()
		.id(("message", id))
		.group(group.clone())
		.relative()
		.flex()
		.items_start()
		.min_h(theme.compact_row)
		.pr(rem(48.))
		.hover(move |style| style.bg(hover))
		.when(view.pending, |row| row.opacity(0.5))
		.when(view.deleted, |row| row.opacity(DELETED_OPACITY))
		.when(highlighted(view, tint), |row| mention_highlight(row, theme))
		.when_some(tint, |row, tint| row.bg(tint))
		.when_some(accent, |row, accent| row.border_l_2().border_color(accent))
		.when_some(tip, |row, tip| row.tooltip(tooltip(tip)))
		.child(
			div()
				.flex_none()
				.w(rem(72.))
				.pr(theme.space_8)
				.text_size(rem(11.))
				.line_height(theme.compact_row)
				.text_color(theme.text_muted)
				.flex()
				.justify_end()
				.child(clock_text(sent_at(message), env.prefs.time_24h)),
		)
		.child(
			div()
				.flex_none()
				.pr(theme.space_4)
				.text_size(theme.font_md)
				.line_height(theme.compact_row)
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(author_color(view, theme))
				.child(view.author_display.clone()),
		)
		.children((!author_tags.is_empty()).then(|| {
			div()
				.flex()
				.flex_none()
				.items_center()
				.gap(theme.space_4)
				.pr(theme.space_4)
				.h(theme.compact_row)
				.children(author_tags)
		}))
		.children(author_badge(view, theme).map(|badge| div().flex_none().pr(theme.space_8).child(badge)))
		.child(div().flex().flex_col().flex_1().min_w_0().children(content));
	interactive(row, view, group, env, cx)
}
