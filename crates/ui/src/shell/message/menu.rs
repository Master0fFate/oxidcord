use std::cell::RefCell;
use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use gpui::{
	AnyElement, App, Global, InteractiveElement as _, IntoElement, MouseDownEvent, ParentElement as _,
	StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_base::TextSelection;
use protocol::{Message, MessageFlags, MessageType, Snowflake};
use state::MessageCommand;

use super::body::snippet;
use super::{Dispatch, ImageTarget, MessageAccess, RowAction, RowEnv, media};
use crate::components::{ContextMenuBuilder, unicode_emoji};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::images::{Images, frame_id};
use crate::shell::plugins::{MenuTarget, PluginMenu as _};
use crate::theme::{Theme, rem};

const QUICK_REACTIONS: [&str; 4] = ["\u{2705}", "\u{1F602}", "\u{1F480}", "\u{1F44D}"];
const QUICK_REACTION_EMOJI: f32 = 26.;
const QUICK_REACTION_TILE: f32 = 46.;
const REMINDER_HOUR: u32 = 9;
const IMAGE_SEARCH: [(&str, &str); 4] = [
	("Google Lens", "https://lens.google.com/uploadbyurl?url="),
	(
		"Bing",
		"https://www.bing.com/images/search?view=detailv2&iss=sbi&q=imgurl:",
	),
	("Yandex", "https://yandex.com/images/search?rpt=imageview&url="),
	("TinEye", "https://tineye.com/search?url="),
];

#[derive(Clone)]
enum Pointed {
	Image {
		link: String,
		fetch: String,
		name: String,
		attachment: Option<u64>,
	},
	Link {
		url: String,
		attachment: Option<u64>,
	},
}

#[derive(Default)]
struct PointedContext {
	media: Option<Pointed>,
	selection: String,
}

struct PointedState(RefCell<PointedContext>);

impl Global for PointedState {}

pub(in crate::shell) fn point_media(
	target: &ImageTarget,
	fetch: &str,
	attachment: Option<u64>,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
	remember(Pointed::Image {
		link: target.url.clone(),
		fetch: fetch.to_owned(),
		name: target.name.clone(),
		attachment,
	})
}

pub(in crate::shell) fn point_link(url: &str) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + use<> {
	remember(Pointed::Link {
		url: url.to_owned(),
		attachment: None,
	})
}

pub(in crate::shell) fn with_link_menu(url: &str, element: AnyElement) -> AnyElement {
	div()
		.on_mouse_down(gpui::MouseButton::Right, point_link(url))
		.child(element)
		.into_any_element()
}

pub(in crate::shell) fn with_attachment_menu(url: &str, attachment: u64, element: AnyElement) -> AnyElement {
	div()
		.on_mouse_down(
			gpui::MouseButton::Right,
			remember(Pointed::Link {
				url: url.to_owned(),
				attachment: Some(attachment),
			}),
		)
		.child(element)
		.into_any_element()
}

fn remember(pointed: Pointed) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
	move |_, _, cx| {
		if let Some(state) = cx.try_global::<PointedState>() {
			state.0.borrow_mut().media = Some(pointed.clone());
		}
	}
}

pub(super) fn capture_pointer(window: &mut Window, cx: &mut App) {
	let selection = if TextSelection::has_selection(window, cx) {
		TextSelection::selected_text(window, cx)
	} else {
		String::new()
	};
	cx.set_global(PointedState(RefCell::new(PointedContext {
		media: None,
		selection,
	})));
}

fn pointed_media(cx: &App) -> Option<Pointed> {
	cx.try_global::<PointedState>()?.0.borrow().media.clone()
}

fn pointed_selection(cx: &App) -> String {
	cx.try_global::<PointedState>()
		.map(|state| state.0.borrow().selection.clone())
		.unwrap_or_default()
}

pub(super) struct Capabilities {
	pub(super) react: bool,
	pub(super) edit: bool,
	pub(super) reply: bool,
	forward: bool,
	thread: bool,
	pin: bool,
	publish: bool,
	delete: bool,
	report: bool,
	apps: bool,
}

fn user_message(message: &Message) -> bool {
	matches!(
		message.kind,
		MessageType::Default
			| MessageType::Reply
			| MessageType::ChatInputCommand
			| MessageType::ContextMenuCommand
	)
}

pub(super) fn capabilities(message: &Message, access: MessageAccess, me: Option<Snowflake>) -> Capabilities {
	let mine = Some(message.author.id) == me;
	let ephemeral = message.flags.contains(MessageFlags::EPHEMERAL);
	let user = user_message(message) && !ephemeral;
	let replyable = user
		|| matches!(
			message.kind,
			MessageType::UserJoin
				| MessageType::PremiumGuildSubscription
				| MessageType::PremiumGuildSubscriptionTier1
				| MessageType::PremiumGuildSubscriptionTier2
				| MessageType::PremiumGuildSubscriptionTier3
		);
	let crossposted = message.flags.contains(MessageFlags::CROSSPOSTED);
	Capabilities {
		react: access.react && !ephemeral,
		edit: mine
			&& matches!(message.kind, MessageType::Default | MessageType::Reply)
			&& !message.is_forward()
			&& message.poll.is_none(),
		reply: access.send && access.read_history && replyable,
		forward: user,
		thread: access.create_threads
			&& !access.in_thread
			&& matches!(message.kind, MessageType::Default | MessageType::Reply)
			&& message.thread.is_none(),
		pin: access.pin && user,
		publish: access.announcement
			&& user && !crossposted
			&& ((mine && access.send) || access.manage_messages),
		delete: !ephemeral && ((mine && user) || access.manage_messages),
		report: !mine && user,
		apps: !ephemeral,
	}
}

#[derive(Clone)]
pub(super) struct MenuSource {
	message: Arc<Message>,
	access: MessageAccess,
	me: Option<Snowflake>,
	dispatch: Dispatch,
	selected: bool,
	selection: usize,
	developer: bool,
	guild: Option<Snowflake>,
}

impl MenuSource {
	pub(super) fn new(message: &Arc<Message>, env: &RowEnv) -> Self {
		Self {
			message: message.clone(),
			access: env.access,
			me: env.me,
			dispatch: env.dispatch.clone(),
			selected: env.selection.contains(&message.id),
			selection: env.selection.len(),
			developer: env.prefs.developer_mode,
			guild: message.guild_id.or(env.lookup.guild),
		}
	}

	pub(super) fn into_menu(self) -> ContextMenuBuilder {
		ContextMenuBuilder::new().lazy(move |cx| self.build(cx))
	}

	fn action(&self, action: RowAction) -> impl Fn(&mut Window, &mut App) + 'static {
		let dispatch = self.dispatch.clone();
		move |window, cx| dispatch(action.clone(), window, cx)
	}

	fn build(&self, cx: &App) -> ContextMenuBuilder {
		let message = &self.message;
		let id = message.id;
		let can = capabilities(message, self.access, self.me);
		let content = match message.forwarded_message() {
			Some(source) => source.content.clone(),
			None => message.content.clone(),
		};
		let mut menu = ContextMenuBuilder::new();
		let selected_text = pointed_selection(cx);
		if !selected_text.is_empty() {
			menu = menu
				.item(cx.t("menu-copy"), Some(IconKind::Copy), move |_, _| {
					copy_text(&selected_text, "selection")
				})
				.separator();
		}
		if can.react {
			let react = self.dispatch.clone();
			menu = menu
				.element(self.quick_reactions(cx))
				.item(cx.t("message-react"), Some(IconKind::Emoji), move |window, cx| {
					react(
						RowAction::OpenEmoji {
							message_id: id,
							at: window.mouse_position(),
						},
						window,
						cx,
					)
				})
				.separator();
		}
		if can.edit {
			menu = menu.item(
				cx.t("message-edit"),
				Some(IconKind::Edit),
				self.action(RowAction::StartEdit(id)),
			);
		}
		if can.reply {
			menu = menu.item(
				cx.t("message-reply"),
				Some(IconKind::Reply),
				self.action(RowAction::Reply(id)),
			);
		}
		if can.forward {
			menu = menu.item(
				cx.t("menu-forward"),
				Some(IconKind::Forward),
				self.action(RowAction::Forward(id)),
			);
		}
		if can.thread {
			menu = menu.item(
				cx.t("menu-create-thread"),
				Some(IconKind::Thread),
				self.action(RowAction::CreateThread(id)),
			);
		}
		menu = menu.separator();
		if !content.is_empty() {
			let copied = content.clone();
			menu = menu.item(cx.t("menu-copy-text"), Some(IconKind::Copy), move |_, _| {
				copy_text(&copied, "message text")
			});
		}
		if can.pin {
			let pinned = message.pinned;
			menu = menu.item(
				cx.t(if pinned { "menu-unpin" } else { "menu-pin" }),
				Some(if pinned { IconKind::Unpin } else { IconKind::Pin }),
				self.action(RowAction::Pin {
					message: id,
					pin: !pinned,
				}),
			);
		}
		menu = menu
			.item(
				cx.t("menu-bookmark"),
				Some(IconKind::Bookmark),
				self.action(RowAction::Save {
					message: id,
					due: None,
				}),
			)
			.submenu(cx.t("menu-reminder"), Some(IconKind::Clock), self.reminders(cx));
		if can.apps {
			menu = menu.extend(self.apps(cx));
		}
		let link = message_link(self.guild, message.channel_id, id);
		menu = menu
			.item(
				cx.t("menu-mark-unread"),
				Some(IconKind::Inbox),
				self.action(RowAction::MarkUnread(id)),
			)
			.item(cx.t("message-copy-link"), Some(IconKind::Link), move |_, _| {
				copy_text(&link, "message link")
			})
			.extend(self.image_search(cx));
		if platform::speech::available() && !content.is_empty() {
			let spoken = snippet(&markdown::parse(&content), usize::MAX);
			menu = menu.item(cx.t("menu-speak"), Some(IconKind::Volume), move |_, _| {
				platform::speech::speak(&spoken)
			});
		}
		if can.publish {
			menu = menu.item(
				cx.t("menu-publish"),
				Some(IconKind::Megaphone),
				self.action(RowAction::Publish(id)),
			);
		}
		if self.access.manage_messages {
			menu = menu.item(
				cx.t(if self.selected {
					"menu-deselect-message"
				} else {
					"menu-select-message"
				}),
				Some(IconKind::Check),
				self.action(RowAction::Select(id)),
			);
		}
		menu = menu.separator();
		if self.access.manage_messages && self.selection > 0 {
			menu = menu.danger_item(
				cx.t_args(
					"menu-delete-selected",
					&[("count", ArgValue::from(self.selection))],
				),
				Some(IconKind::Trash),
				self.action(RowAction::DeleteSelected),
			);
		}
		if can.delete {
			menu = menu.danger_item(
				cx.t("menu-delete"),
				Some(IconKind::Trash),
				self.action(RowAction::Delete(id)),
			);
		}
		if can.report {
			menu = menu.danger_item(
				cx.t("menu-report"),
				Some(IconKind::Flag),
				self.action(RowAction::Report),
			);
		}
		menu = menu.extend(self.media_actions(cx));
		if self.developer {
			menu = menu
				.separator()
				.item(cx.t("message-copy-id"), Some(IconKind::Profile), move |_, _| {
					copy_text(&id.to_string(), "message id")
				});
		}
		let target = MenuTarget::message(message, self.guild, self.me);
		menu.extend(pointed_plugin_items(&target, cx))
			.plugin_items("message-context-menu", target, cx)
	}

	fn quick_reactions(&self, cx: &App) -> impl Fn(&mut Window, &mut App) -> AnyElement + 'static {
		let data = super::super::community::data_handle(cx);
		let mut picks: Vec<QuickReaction> = data
			.read(cx)
			.state()
			.recent_emoji()
			.iter()
			.take(QUICK_REACTIONS.len())
			.map(|emoji| QuickReaction {
				key: match emoji.id {
					Some(id) => format!("{}:{id}", emoji.name),
					None => emoji.name.clone(),
				},
				glyph: emoji.name.clone(),
				custom: emoji.id.map(|id| (id.get(), emoji.animated)),
				reacted: false,
			})
			.collect();
		for glyph in QUICK_REACTIONS {
			if picks.len() == QUICK_REACTIONS.len() {
				break;
			}
			if !picks.iter().any(|pick| pick.key == glyph) {
				picks.push(QuickReaction {
					key: glyph.to_owned(),
					glyph: glyph.to_owned(),
					custom: None,
					reacted: false,
				});
			}
		}
		for pick in &mut picks {
			pick.reacted = self
				.message
				.reactions
				.iter()
				.any(|reaction| reaction.me && media::emoji_key(&reaction.emoji) == pick.key);
		}
		let (channel_id, message_id) = (self.message.channel_id, self.message.id);
		let dispatch = self.dispatch.clone();
		move |_, cx| {
			let theme = cx.global::<Theme>();
			div()
				.flex()
				.justify_between()
				.w_full()
				.gap(theme.space_8)
				.py(theme.space_4)
				.children(picks.iter().enumerate().map(|(index, pick)| {
					let dispatch = dispatch.clone();
					let emoji = pick.key.clone();
					let add = !pick.reacted;
					div()
						.id(("quick-reaction", index))
						.flex()
						.items_center()
						.justify_center()
						.size(rem(QUICK_REACTION_TILE))
						.rounded(theme.radius_md)
						.bg(theme.button_secondary)
						.hover(|style| style.bg(theme.modifier_selected))
						.on_click(move |_, window, cx| {
							dispatch(
								RowAction::React {
									channel_id,
									message_id,
									emoji: emoji.clone(),
									add,
								},
								window,
								cx,
							)
						})
						.child(pick.visual(cx))
				}))
				.into_any_element()
		}
	}

	fn reminders(&self, cx: &App) -> ContextMenuBuilder {
		let choices: [(&str, DueTime); 4] = [
			("menu-reminder-30m", || Utc::now() + chrono::Duration::minutes(30)),
			("menu-reminder-1h", || Utc::now() + chrono::Duration::hours(1)),
			("menu-reminder-3h", || Utc::now() + chrono::Duration::hours(3)),
			("menu-reminder-tomorrow", tomorrow_morning),
		];
		let id = self.message.id;
		choices
			.into_iter()
			.fold(ContextMenuBuilder::new(), |menu, (label, due)| {
				let remind = self.dispatch.clone();
				menu.item(cx.t(label), None, move |window, cx| {
					remind(
						RowAction::Save {
							message: id,
							due: Some(due()),
						},
						window,
						cx,
					)
				})
			})
	}

	fn apps(&self, cx: &App) -> ContextMenuBuilder {
		let data = super::super::community::data_handle(cx);
		let commands: Vec<MessageCommand> = data
			.read(cx)
			.state()
			.message_commands(self.guild, self.message.channel_id);
		if commands.is_empty() {
			return ContextMenuBuilder::new();
		}
		let several = commands
			.windows(2)
			.any(|pair| pair[0].application_name != pair[1].application_name);
		let id = self.message.id;
		let entries = commands
			.into_iter()
			.fold(ContextMenuBuilder::new(), |menu, found| {
				let label = if several {
					format!("{} · {}", found.command.name, found.application_name)
				} else {
					found.command.name.clone()
				};
				let command = Box::new(found.command);
				menu.item(
					label,
					None,
					self.action(RowAction::RunCommand { message: id, command }),
				)
			});
		ContextMenuBuilder::new().submenu(cx.t("menu-apps"), Some(IconKind::Apps), entries)
	}

	fn image_search(&self, cx: &App) -> ContextMenuBuilder {
		let Some(Pointed::Image { link, .. }) = pointed_media(cx) else {
			return ContextMenuBuilder::new();
		};
		let encoded: String = url::form_urlencoded::byte_serialize(link.as_bytes()).collect();
		let engines = IMAGE_SEARCH
			.iter()
			.fold(ContextMenuBuilder::new(), |menu, (name, prefix)| {
				menu.item(
					*name,
					None,
					self.action(RowAction::OpenLink(format!("{prefix}{encoded}"))),
				)
			});
		ContextMenuBuilder::new().submenu(cx.t("menu-search-image"), Some(IconKind::Search), engines)
	}

	fn media_actions(&self, cx: &App) -> ContextMenuBuilder {
		match pointed_media(cx) {
			Some(Pointed::Image {
				link, fetch, name, ..
			}) => {
				let save = fetch.clone();
				ContextMenuBuilder::new()
					.separator()
					.item(cx.t("menu-copy-image"), Some(IconKind::Image), move |_, cx| {
						cx.global::<Images>().copy_image(&fetch)
					})
					.item(cx.t("menu-save-image"), Some(IconKind::Download), move |_, cx| {
						cx.global::<Images>().save_image(&save, name.clone())
					})
					.extend(self.link_items(link, "menu-copy-image-link", "menu-open-image-link", cx))
			}
			Some(Pointed::Link { url, .. }) => self.link_items(url, "menu-copy-link", "menu-open-link", cx),
			None => ContextMenuBuilder::new(),
		}
	}

	fn link_items(&self, link: String, copy: &str, open: &str, cx: &App) -> ContextMenuBuilder {
		let copied = link.clone();
		ContextMenuBuilder::new()
			.separator()
			.item(cx.t(copy), Some(IconKind::Link), move |_, _| {
				copy_text(&copied, "link")
			})
			.item(
				cx.t(open),
				Some(IconKind::ExternalLink),
				self.action(RowAction::OpenLink(link)),
			)
	}
}

fn pointed_plugin_items(base: &MenuTarget, cx: &App) -> ContextMenuBuilder {
	let mut menu = ContextMenuBuilder::new();
	let selection = pointed_selection(cx);
	if !selection.is_empty() {
		menu = menu.plugin_items("selection-context-menu", base.clone().selecting(selection), cx);
	}
	let (target, attachment) = match pointed_media(cx) {
		Some(Pointed::Image { link, attachment, .. }) => {
			let image = base.clone().image(&link);
			menu = menu.plugin_items("image-context-menu", image.clone(), cx);
			(image, attachment)
		}
		Some(Pointed::Link { url, attachment }) => {
			let link = base.clone().with_url(url);
			menu = menu.plugin_items("link-context-menu", link.clone(), cx);
			(link, attachment)
		}
		None => return menu,
	};
	match attachment {
		Some(id) => menu.plugin_items("attachment-context-menu", target.attachment(Snowflake(id)), cx),
		None => menu,
	}
}

struct QuickReaction {
	key: String,
	glyph: String,
	custom: Option<(u64, bool)>,
	reacted: bool,
}

impl QuickReaction {
	fn visual(&self, cx: &App) -> AnyElement {
		match self.custom {
			Some((id, animated)) => {
				let pixels = (QUICK_REACTION_EMOJI * 2.) as u32;
				let url = ::media::cdn::emoji_url(id, animated, pixels);
				let image = cx.global::<Images>().get(&url, ::media::SizeHint::square(pixels));
				div()
					.size(rem(QUICK_REACTION_EMOJI))
					.when_some(image, |slot, image| {
						slot.child(gpui::img(image).id(frame_id(&url)).size_full())
					})
					.into_any_element()
			}
			None => unicode_emoji(&self.glyph, rem(QUICK_REACTION_EMOJI), cx),
		}
	}
}

type DueTime = fn() -> DateTime<Utc>;

fn tomorrow_morning() -> DateTime<Utc> {
	let tomorrow = Local::now().date_naive() + chrono::Duration::days(1);
	tomorrow
		.and_hms_opt(REMINDER_HOUR, 0, 0)
		.and_then(|time| time.and_local_timezone(Local).earliest())
		.map_or_else(
			|| Utc::now() + chrono::Duration::days(1),
			|time| time.with_timezone(&Utc),
		)
}

fn message_link(guild: Option<Snowflake>, channel_id: Snowflake, id: Snowflake) -> String {
	let scope = guild.map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
	format!("https://discord.com/channels/{scope}/{channel_id}/{id}")
}

fn copy_text(text: &str, what: &str) {
	if let Err(error) = platform::clipboard::write_text(text) {
		tracing::warn!(%error, what, "could not copy to the clipboard");
	}
}
