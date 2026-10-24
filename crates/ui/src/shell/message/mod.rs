mod body;
mod media;
mod reaction_tip;

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
use protocol::{Message, MessageType, Snowflake};
use state::{MessageRow, MessageView, RenderedMessage};

pub use self::body::snippet;
use self::body::{BodyEnv, render_body};
pub use self::reaction_tip::ReactionRequests;
use super::community;
use super::plugins::{PluginMenu as _, author_badge, message_actions, message_extras};
use crate::components::{Avatar, ContextMenuBuilder, IconButton, tooltip};
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

#[derive(Clone, Copy)]
pub struct Prefs {
	pub inline_media: bool,
	pub link_previews: bool,
	pub show_reactions: bool,
	pub time_24h: bool,
	pub font_scale: f32,
	pub developer_mode: bool,
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

pub enum RowAction {
	OpenLink(String),
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
}

#[derive(Clone, Copy, Default)]
pub struct MessageAccess {
	pub manage_messages: bool,
	pub create_threads: bool,
	pub in_thread: bool,
	pub guild: bool,
}

#[derive(Clone)]
pub struct Editing {
	pub message: Snowflake,
	pub input: Entity<TextareaState>,
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

fn reply_header(view: &MessageView, env: &RowEnv, cx: &App) -> Option<AnyElement> {
	let theme = env.theme;
	let message = &view.message;
	message.message_reference.as_ref()?.message_id?;
	let left = |child: AnyElement| {
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
		));
	};
	let author = &referenced.author;
	let name = env.lookup.user(author.id.get());
	let document = markdown::parse(&referenced.content);
	let text = if referenced.content.is_empty() {
		cx.t("message-reply-attachment").to_string()
	} else {
		snippet(&document, SNIPPET_LENGTH)
	};
	let target = referenced.id;
	let dispatch = env.dispatch.clone();
	Some(left(
		div()
			.id(("reply", message.id.get()))
			.flex()
			.items_center()
			.gap(theme.space_4)
			.min_w_0()
			.cursor_pointer()
			.on_click(move |_, window, cx| dispatch(RowAction::JumpTo(target), window, cx))
			.child(
				Avatar::new(name.clone())
					.size(rem(16.))
					.url(user_avatar(author, 32)),
			)
			.child(
				div()
					.flex_none()
					.text_size(theme.font_sm)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_secondary)
					.child(format!("@{name}")),
			)
			.child(
				div()
					.flex_1()
					.min_w_0()
					.truncate()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(text),
			)
			.into_any_element(),
	))
}

fn menu_for(view: &MessageView, env: &RowEnv, cx: &App) -> ContextMenuBuilder {
	let message = &view.message;
	let id = message.id;
	let channel_id = message.channel_id;
	let guild = message.guild_id;
	let content = message.content.clone();
	let mine = Some(message.author.id) == env.me;
	let reply = env.dispatch.clone();
	let edit = env.dispatch.clone();
	let delete = env.dispatch.clone();
	let mut menu =
		ContextMenuBuilder::new().item(cx.t("message-reply"), Some(IconKind::Reply), move |window, cx| {
			reply(RowAction::Reply(id), window, cx)
		});
	if mine {
		menu = menu.item(cx.t("message-edit"), Some(IconKind::Edit), move |window, cx| {
			edit(RowAction::StartEdit(id), window, cx)
		});
	}
	menu = menu
		.separator()
		.item(cx.t("menu-copy-text"), Some(IconKind::Copy), move |_, _| {
			if let Err(error) = platform::clipboard::write_text(&content) {
				tracing::warn!(%error, "could not copy the message text");
			}
		})
		.item(cx.t("menu-copy-link"), Some(IconKind::Link), move |_, _| {
			let scope = guild.map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
			let link = format!("https://discord.com/channels/{scope}/{channel_id}/{id}");
			if let Err(error) = platform::clipboard::write_text(&link) {
				tracing::warn!(%error, "could not copy the message link");
			}
		})
		.item(cx.t("menu-forward"), Some(IconKind::Forward), {
			let forward = env.dispatch.clone();
			move |window, cx| forward(RowAction::Forward(id), window, cx)
		});
	if env.prefs.developer_mode {
		menu = menu.item(cx.t("menu-copy-id"), Some(IconKind::Copy), move |_, _| {
			if let Err(error) = platform::clipboard::write_text(&id.to_string()) {
				tracing::warn!(%error, "could not copy the message id");
			}
		});
	}
	menu = menu.separator();
	let access = env.access;
	if access.manage_messages || !access.guild {
		let pinned = message.pinned;
		let pin = env.dispatch.clone();
		menu = menu.item(
			cx.t(if pinned { "menu-unpin" } else { "menu-pin" }),
			Some(if pinned { IconKind::Unpin } else { IconKind::Pin }),
			move |window, cx| {
				pin(
					RowAction::Pin {
						message: id,
						pin: !pinned,
					},
					window,
					cx,
				)
			},
		);
	}
	let threadable = matches!(message.kind, MessageType::Default | MessageType::Reply);
	if access.create_threads && !access.in_thread && threadable && message.thread.is_none() {
		let create = env.dispatch.clone();
		menu = menu.item(
			cx.t("menu-create-thread"),
			Some(IconKind::NewThread),
			move |window, cx| create(RowAction::CreateThread(id), window, cx),
		);
	}
	let mark_unread = env.dispatch.clone();
	menu = menu.item(
		cx.t("menu-mark-unread"),
		Some(IconKind::Inbox),
		move |window, cx| mark_unread(RowAction::MarkUnread(id), window, cx),
	);
	if access.manage_messages {
		let select = env.dispatch.clone();
		let selected = env.selection.contains(&id);
		menu = menu.item(
			cx.t(if selected {
				"menu-deselect-message"
			} else {
				"menu-select-message"
			}),
			Some(IconKind::Check),
			move |window, cx| select(RowAction::Select(id), window, cx),
		);
		if !env.selection.is_empty() {
			let bulk = env.dispatch.clone();
			menu = menu.danger_item(
				cx.t_args(
					"menu-delete-selected",
					&[("count", ArgValue::from(env.selection.len()))],
				),
				Some(IconKind::Trash),
				move |window, cx| bulk(RowAction::DeleteSelected, window, cx),
			);
		}
	}
	if mine || access.manage_messages {
		menu = menu.danger_item(cx.t("menu-delete"), Some(IconKind::Trash), move |window, cx| {
			delete(RowAction::Delete(id), window, cx)
		});
	}
	if !mine {
		let report = env.dispatch.clone();
		menu = menu.danger_item(cx.t("menu-report"), Some(IconKind::Flag), move |window, cx| {
			report(RowAction::Report, window, cx)
		});
	}
	menu.plugin_items("message-context-menu", id.to_string(), cx)
}

fn toolbar(view: &MessageView, group: SharedString, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let message = &view.message;
	let id = message.id;
	let key = id.get();
	let mine = Some(message.author.id) == env.me;
	let react = env.dispatch.clone();
	let reply = env.dispatch.clone();
	let edit = env.dispatch.clone();
	let more_menu = menu_for(view, env, cx).build();
	div()
		.invisible()
		.group_hover(group, |style| style.visible())
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
		.child(
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
		.child(
			IconButton::new(("message-reply", key), IconKind::Reply)
				.filled_hover()
				.hit_size(rem(28.))
				.icon_size(rem(18.))
				.tooltip(cx.t("message-reply"))
				.on_click(move |_, window, cx| reply(RowAction::Reply(id), window, cx)),
		)
		.when(mine, |bar| {
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
		.children(message_actions(id, cx))
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

fn content_column(view: &MessageView, env: &RowEnv, cx: &App) -> Vec<AnyElement> {
	let message = &view.message;
	let theme = env.theme;
	let mut out: Vec<AnyElement> = Vec::new();
	let body_env = BodyEnv::new(
		theme,
		env.lookup,
		env.dispatch,
		message.id.get(),
		env.revealed,
		env.prefs,
	);
	if let Some(editing) = env.editing.filter(|editing| editing.message == message.id) {
		out.push(editor(editing, env, cx));
	} else if !message.content.is_empty() {
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
		out.push(render_body(&document, &body_env, cx));
	}
	let body_env = BodyEnv::new(
		theme,
		env.lookup,
		env.dispatch,
		message.id.get(),
		env.revealed,
		env.prefs,
	);
	out.extend(media::attachments(message, &body_env, env.prefs.inline_media, cx));
	if !message.flags.contains(protocol::MessageFlags::SUPPRESS_EMBEDS) {
		out.extend(media::embeds(message, &body_env, env.prefs.link_previews, cx));
	}
	out.extend(media::stickers(message, &body_env, cx));
	out.extend(thread_chip(message, env, cx));
	out.extend(super::community::invites::cards(
		&message.content,
		env.lookup.state,
		env.theme,
		cx,
	));
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
	out.extend(message_extras(view, theme, cx));
	out
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
						.appearance(false)
						.bordered(false)
						.p_0(),
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
	let row = row
		.when(env.selection.contains(&id), |row| {
			row.bg(crate::theme::with_alpha(env.theme.brand, 0.14))
		})
		.on_click(move |event, window, cx| {
			if selectable && (event.modifiers().shift || selecting) {
				dispatch(RowAction::Select(id), window, cx);
			}
		});
	row.child(toolbar(view, group, env, cx))
		.context_menu(menu_for(view, env, cx).build())
		.into_any_element()
}

fn message_row(view: &MessageView, env: &RowEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let message = &view.message;
	let id = message.id.get();
	let group: SharedString = format!("message-{id}").into();
	let reply = reply_header(view, env, cx);
	let has_reply = reply.is_some();
	let content = content_column(view, env, cx);
	let container = div()
		.id(("message", id))
		.group(group.clone())
		.relative()
		.flex()
		.flex_col()
		.pl(rem(72.))
		.pr(rem(48.))
		.py(rem(2.))
		.hover(|style| style.bg(message_hover_background()))
		.when(view.pending, |row| row.opacity(0.5))
		.when(view.deleted, |row| row.opacity(DELETED_OPACITY))
		.when(view.failed, |row| row.text_color(theme.danger));
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
	let profile = env.dispatch.clone();
	let avatar_profile = env.dispatch.clone();
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
						.url(user_avatar(&message.author, 128))
						.color(avatar_color),
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
	let content = content_column(view, env, cx);
	let row = div()
		.id(("message", id))
		.group(group.clone())
		.relative()
		.flex()
		.items_start()
		.min_h(theme.compact_row)
		.pr(rem(48.))
		.hover(|style| style.bg(message_hover_background()))
		.when(view.pending, |row| row.opacity(0.5))
		.when(view.deleted, |row| row.opacity(DELETED_OPACITY))
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
		.children(author_badge(view, theme).map(|badge| div().flex_none().pr(theme.space_8).child(badge)))
		.child(div().flex().flex_col().flex_1().min_w_0().children(content));
	interactive(row, view, group, env, cx)
}
