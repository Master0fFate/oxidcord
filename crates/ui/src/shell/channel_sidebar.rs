use gpui::{
	AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FontWeight,
	InteractiveElement as _, IntoElement, MouseDownEvent, ParentElement as _, Pixels, Point, Render,
	SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
	transparent_black,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{ChannelType, Permissions, ScheduledEventStatus, Snowflake};
use session::Command;
use state::{ChannelRow, ChannelTreeNode, JoinBlock, PluginBadge};

use crate::components::{IconButton, MentionBadge, SectionHeader, badge};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind, channel_icon};
use crate::theme::{Theme, rem};

use self::reorder::{DragPreview, DraggedChannel, Drop, Group, Slot, rearranged};
use super::community::access::GuildAccess;
use super::community::act;
use super::community::events::{self, CREATE_PERMISSIONS};
use super::community::menus::{ChannelFacts, channel_menu, guild_menu};
use super::plugins::{self, MenuTarget, PluginMenu as _};
use super::server_settings::channel as channel_modals;
use super::voice::{Participant, participant_row, participants};

mod reorder;

const AVATAR_PIXELS: u32 = 48;

pub enum SidebarEvent {
	SelectChannel(Snowflake),
	OpenVoiceChat(Snowflake),
	VoiceUser { user: Snowflake, at: Point<Pixels> },
}

#[derive(PartialEq)]
struct ChannelItem {
	id: Snowflake,
	position: i64,
	kind: ChannelType,
	name: SharedString,
	thread: bool,
	last_thread: bool,
	joined: bool,
	icon: IconKind,
	voice_channel: bool,
	event_live: bool,
	can_chat: bool,
	can_invite: bool,
	can_manage: bool,
	selectable: bool,
	locked: bool,
	unread: bool,
	muted: bool,
	mentions: u32,
	voice: Vec<Participant>,
	topic: Option<SharedString>,
	user_limit: u32,
	occupants: u32,
	voice_status: Option<SharedString>,
	join_block: Option<JoinBlock>,
	badges: Vec<PluginBadge>,
}

#[derive(PartialEq)]
struct CategoryItem {
	id: Option<Snowflake>,
	position: i64,
	name: SharedString,
	collapsed: bool,
	can_create: bool,
	channels: Vec<ChannelItem>,
}

pub struct ChannelSidebar {
	data: Entity<Data>,
	guild: Option<Snowflake>,
	name: SharedString,
	selected: Option<Snowflake>,
	categories: Vec<CategoryItem>,
	access: GuildAccess,
	invite_channel: Option<Snowflake>,
	unread: bool,
	muted: bool,
	events: usize,
	events_live: bool,
	hide_muted: bool,
}

impl EventEmitter<SidebarEvent> for ChannelSidebar {}

fn selectable(kind: ChannelType, locked: bool) -> bool {
	!locked && kind != ChannelType::GuildCategory
}

fn is_thread(kind: ChannelType) -> bool {
	matches!(
		kind,
		ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::AnnouncementThread
	)
}

impl ChannelSidebar {
	pub fn new(data: Entity<Data>) -> Self {
		Self {
			data,
			guild: None,
			name: SharedString::default(),
			selected: None,
			categories: Vec::new(),
			access: GuildAccess::default(),
			invite_channel: None,
			unread: false,
			muted: false,
			events: 0,
			events_live: false,
			hide_muted: false,
		}
	}

	pub fn show(&mut self, guild: Option<Snowflake>, selected: Option<Snowflake>, cx: &mut Context<Self>) {
		self.guild = guild;
		self.selected = selected;
		self.refresh(cx);
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		let Some(guild) = self.guild else {
			return;
		};
		let data = self.data.read(cx);
		let state = data.state();
		let name: SharedString = state
			.guild(guild)
			.map(|guild| guild.name.clone().into())
			.unwrap_or_default();
		let mut nodes = state.guild_channel_tree(guild);
		if !data.session().plugins().reveals_hidden_channels() {
			nodes.retain_mut(drop_hidden);
		}
		let hide_muted = state.hides_muted_channels(guild);
		if hide_muted {
			let selected = self.selected;
			nodes.retain_mut(|node| drop_muted(node, selected));
		}
		let categories: Vec<CategoryItem> = nodes
			.iter()
			.map(|node| category_item(node, guild, &state))
			.collect();
		let access = GuildAccess::of(&state, guild);
		let invite_channel = nodes
			.iter()
			.flat_map(|node| node.children.iter())
			.find(|row| !row.locked && row.kind == ChannelType::GuildText)
			.map(|row| row.id);
		let summary = state.guild_summary(guild);
		let unread = summary
			.as_ref()
			.is_some_and(|summary| summary.unread || summary.mentions > 0);
		let muted = summary.is_some_and(|summary| summary.muted);
		let events = state.scheduled_events(guild);
		let (event_count, events_live) = (
			events.len(),
			events
				.iter()
				.any(|event| event.status == ScheduledEventStatus::Active),
		);
		drop(state);
		let changed = name != self.name
			|| categories != self.categories
			|| access != self.access
			|| invite_channel != self.invite_channel
			|| unread != self.unread
			|| muted != self.muted
			|| event_count != self.events
			|| events_live != self.events_live
			|| hide_muted != self.hide_muted;
		if changed {
			self.name = name;
			self.categories = categories;
			self.access = access;
			self.invite_channel = invite_channel;
			self.unread = unread;
			self.muted = muted;
			self.events = event_count;
			self.events_live = events_live;
			self.hide_muted = hide_muted;
			cx.notify();
		}
	}

	fn toggle(&self, category: Snowflake, cx: &mut Context<Self>) {
		if let Some(guild) = self.guild {
			self.data.read(cx).send(Command::ToggleCategory {
				guild_id: guild,
				category_id: category,
			});
		}
	}

	fn header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let title = div()
			.flex()
			.items_center()
			.justify_between()
			.w_full()
			.h(theme.guild_header_height)
			.px(theme.space_16)
			.text_size(theme.font_md)
			.font_weight(FontWeight::SEMIBOLD)
			.text_color(theme.header_primary)
			.child(div().min_w_0().truncate().child(self.name.clone()))
			.child(
				Icon::new(IconKind::ChevronDown)
					.size(rem(18.))
					.color(theme.header_primary),
			);
		let Some(guild) = self.guild else {
			return div().into_any_element();
		};
		let menu = guild_menu(
			guild,
			self.invite_channel,
			self.unread,
			self.muted,
			self.hide_muted,
			&self.access,
			cx,
		)
		.plugin_items("server-context-menu", MenuTarget::server(guild), cx);
		let click_menu = guild_menu(
			guild,
			self.invite_channel,
			self.unread,
			self.muted,
			self.hide_muted,
			&self.access,
			cx,
		)
		.plugin_items("server-context-menu", MenuTarget::server(guild), cx);
		div()
			.id("guild-header")
			.relative()
			.flex_none()
			.h(theme.guild_header_height)
			.child(
				Button::new("guild-header-button")
					.ghost()
					.size_full()
					.child(title)
					.dropdown_menu(click_menu.build()),
			)
			.child(
				div()
					.absolute()
					.left_0()
					.right_0()
					.bottom_0()
					.h(rem(1.))
					.bg(theme.divider),
			)
			.context_menu(menu.build())
			.into_any_element()
	}

	fn events_row(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let guild = self.guild?;
		if self.events == 0 && !self.access.has_any(CREATE_PERMISSIONS) {
			return None;
		}
		let hover_background = theme.modifier_hover;
		let label = cx.t_args("events-count", &[("count", ArgValue::from(self.events))]);
		Some(
			div()
				.id("guild-events")
				.flex()
				.items_center()
				.gap(rem(6.))
				.h(theme.channel_row_height)
				.mt(theme.space_8)
				.mx(theme.space_8)
				.px(theme.space_8)
				.rounded(theme.radius_md)
				.cursor_pointer()
				.text_size(theme.font_md)
				.font_weight(FontWeight::MEDIUM)
				.text_color(theme.text_muted)
				.hover(move |style| style.bg(hover_background))
				.on_click(move |_, window, cx| events::open(window, cx, guild))
				.child(
					Icon::new(IconKind::Calendar)
						.size(rem(20.))
						.color(theme.text_muted),
				)
				.child(div().flex_1().min_w_0().truncate().child(label))
				.when(self.events_live, |row| {
					row.child(
						div()
							.flex_none()
							.size(theme.space_8)
							.rounded_full()
							.bg(theme.danger),
					)
				})
				.into_any_element(),
		)
	}

	fn category(&self, category: &CategoryItem, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let rows: Vec<AnyElement> = category
			.channels
			.iter()
			.filter(|channel| {
				!category.collapsed
					|| Some(channel.id) == self.selected
					|| channel.unread
					|| !channel.voice.is_empty()
			})
			.map(|channel| self.channel_row(channel, theme, cx))
			.collect();
		let header = category.id.map(|id| {
			let mut section = SectionHeader::new(("category", id.get()), category.name.clone())
				.collapsible(category.collapsed);
			if let Some(guild) = self.guild.filter(|_| category.can_create) {
				section = section
					.trailing(
						IconButton::new(("category-add", id.get()), IconKind::Plus)
							.icon_size(rem(16.))
							.hit_size(rem(18.))
							.tooltip(cx.t("tooltip-create-channel"))
							.on_click(move |_, window, cx| {
								channel_modals::open_create(window, cx, guild, Some(id));
							}),
					)
					.trailing_on_hover();
			}
			let wrapper = div()
				.id(("category-header", id.get()))
				.relative()
				.pt(theme.space_16)
				.child(section.on_click(cx.listener(move |this, _, _, cx| this.toggle(id, cx))));
			if self.access.has(Permissions::MANAGE_CHANNELS) {
				let dragged = DraggedChannel {
					id,
					category: true,
					name: category.name.clone(),
					icon: IconKind::ChevronDown,
				};
				wrapper
					.on_drag(dragged, |dragged, _, _, cx| cx.new(|_| DragPreview::new(dragged)))
					.child(self.header_drop_zone(id, theme, cx))
					.into_any_element()
			} else {
				wrapper.into_any_element()
			}
		});
		let block = div()
			.id(("category-block", category.id.map_or(0, Snowflake::get)))
			.flex()
			.flex_col()
			.children(header)
			.children(rows);
		if self.access.has(Permissions::MANAGE_CHANNELS) {
			let end = category.id;
			block
				.on_drop(cx.listener(move |this, dragged: &DraggedChannel, _, cx| {
					this.drop_dragged(dragged, Drop::EndOf(end), cx)
				}))
				.into_any_element()
		} else {
			block.into_any_element()
		}
	}

	fn header_drop_zone(&self, category: Snowflake, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let brand = theme.brand;
		div()
			.id(("category-drop", category.get()))
			.absolute()
			.bottom_0()
			.left_0()
			.right_0()
			.h(rem(24.))
			.rounded(theme.radius_sm)
			.border_color(transparent_black())
			.drag_over::<DraggedChannel>(move |style, dragged, _, _| {
				if dragged.category {
					style.border_t_2().border_color(brand)
				} else {
					style.border_2().border_color(brand)
				}
			})
			.on_drop(cx.listener(move |this, dragged: &DraggedChannel, _, cx| {
				let drop = if dragged.category {
					Drop::BeforeCategory(category)
				} else {
					Drop::StartOf(category)
				};
				this.drop_dragged(dragged, drop, cx)
			}))
			.into_any_element()
	}

	fn channel_drop_zone(&self, channel: Snowflake, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let brand = theme.brand;
		div()
			.id(("channel-drop", channel.get()))
			.absolute()
			.top_0()
			.left_0()
			.size_full()
			.border_color(transparent_black())
			.drag_over::<DraggedChannel>(move |style, _, _, _| style.border_t_2().border_color(brand))
			.on_drop(cx.listener(move |this, dragged: &DraggedChannel, _, cx| {
				this.drop_dragged(dragged, Drop::BeforeChannel(channel), cx)
			}))
			.into_any_element()
	}

	fn drop_dragged(&mut self, dragged: &DraggedChannel, drop: Drop, cx: &mut Context<Self>) {
		let Some(guild) = self.guild else {
			return;
		};
		let positions = rearranged(&self.groups(guild, cx), dragged, drop);
		if !positions.is_empty() {
			self.data.read(cx).send(Command::ReorderChannels {
				guild_id: guild,
				positions,
			});
		}
	}

	fn groups(&self, guild: Snowflake, cx: &App) -> Vec<Group> {
		self.data
			.read(cx)
			.state()
			.guild_channel_tree(guild)
			.iter()
			.map(|node| Group {
				id: node.category.as_ref().map(|category| category.id),
				position: node.category.as_ref().map_or(0, |category| category.position),
				channels: node
					.children
					.iter()
					.filter(|row| !is_thread(row.kind))
					.map(|row| Slot {
						id: row.id,
						position: row.position,
					})
					.collect(),
			})
			.collect()
	}

	fn channel_row(&self, channel: &ChannelItem, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = channel.id;
		let key = id.get();
		let selected = Some(id) == self.selected;
		let (rest, hover) = if selected {
			(theme.interactive_active, theme.interactive_active)
		} else if channel.unread {
			(theme.header_primary, theme.header_primary)
		} else if channel.muted || channel.locked {
			(theme.interactive_muted, theme.interactive_normal)
		} else {
			(theme.text_muted, theme.interactive_hover)
		};
		let group: SharedString = format!("channel-{key}").into();
		let glyph = div()
			.relative()
			.flex_none()
			.size(rem(20.))
			.child(
				Icon::new(channel.icon)
					.size(rem(20.))
					.color(rest)
					.group_hover(group.clone(), hover),
			)
			.when(
				channel.locked || channel.join_block == Some(JoinBlock::NoPermission),
				|glyph| {
					glyph.child(
						div()
							.absolute()
							.right(rem(-3.))
							.bottom(rem(-3.))
							.flex()
							.items_center()
							.justify_center()
							.size(rem(11.))
							.rounded_full()
							.bg(theme.background_tertiary)
							.child(Icon::new(IconKind::Lock).size(rem(9.)).color(rest)),
					)
				},
			);
		let live_label = cx.t("voice-live");
		let participants: Vec<AnyElement> = channel
			.voice
			.iter()
			.map(|member| {
				let user = member.user;
				participant_row(
					member,
					theme,
					live_label.clone(),
					cx.listener(move |_, event: &MouseDownEvent, _, cx| {
						cx.emit(SidebarEvent::VoiceUser {
							user,
							at: event.position,
						})
					}),
				)
			})
			.collect();
		let facts = ChannelFacts {
			id,
			kind: channel.kind,
			name: channel.name.to_string(),
			unread: channel.unread,
			muted: channel.muted,
			joined: channel.joined,
			can_invite: channel.can_invite,
			can_manage: channel.can_manage,
		};
		let menu = channel_menu(self.guild, facts)
			.plugin_items("channel-context-menu", MenuTarget::channel(self.guild, id), cx)
			.build();
		let thread = channel.thread;
		let hover_background = theme.modifier_hover;
		let can_select = channel.selectable;
		let voice_channel = channel.voice_channel;
		let chat_group = group.clone();
		let chat_button = channel.can_chat.then(|| {
			div()
				.flex_none()
				.invisible()
				.when(selected, |slot| slot.visible())
				.group_hover(chat_group, |style| style.visible())
				.child(
					IconButton::new(("channel-chat", key), IconKind::Chat)
						.icon_size(rem(16.))
						.hit_size(rem(24.))
						.tooltip(cx.t("channel-open-chat"))
						.on_click(cx.listener(move |_, _, _, cx| {
							cx.stop_propagation();
							cx.emit(SidebarEvent::OpenVoiceChat(id));
						})),
				)
		});
		let reorder = self.access.has(Permissions::MANAGE_CHANNELS) && !thread;
		let dragged = reorder.then(|| DraggedChannel {
			id,
			category: false,
			name: channel.name.clone(),
			icon: channel.icon,
		});
		let drop_zone = reorder.then(|| self.channel_drop_zone(id, theme, cx));
		let row = div()
			.id(("channel", key))
			.group(group)
			.relative()
			.flex()
			.items_center()
			.gap(rem(6.))
			.h(theme.channel_row_height)
			.mx(theme.space_8)
			.when(thread, |row| row.ml(theme.space_32 + theme.space_8))
			.px(theme.space_8)
			.rounded(theme.radius_md)
			.when(can_select, |row| row.cursor_pointer())
			.when(!can_select, |row| row.cursor_not_allowed())
			.text_size(theme.font_md)
			.text_color(rest)
			.when(selected, |row| row.bg(theme.modifier_selected))
			.when(!selected, |row| {
				row.hover(move |style| style.bg(hover_background))
			})
			.on_aux_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
				if event.is_middle_click() {
					plugins::channel_click(this.guild, id, event, cx);
				}
			}))
			.when(can_select, |row| {
				row.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
					if plugins::channel_click(this.guild, id, event, cx) {
						return;
					}
					if thread {
						act(window, cx, move |community, _, cx| community.open_thread(id, cx));
						return;
					}
					if voice_channel {
						this.data.read(cx).send(Command::JoinVoice {
							guild_id: this.guild,
							channel_id: id,
						});
					}
					cx.emit(SidebarEvent::SelectChannel(id))
				}))
			})
			.when(channel.unread && !selected, |row| {
				row.child(
					div()
						.absolute()
						.left(rem(-8.))
						.top(rem(13.))
						.w(rem(4.))
						.h(rem(8.))
						.rounded_r(rem(4.))
						.bg(theme.unread_pill),
				)
			})
			.child(glyph)
			.child(
				div()
					.flex_1()
					.min_w_0()
					.truncate()
					.font_weight(if channel.unread || selected {
						FontWeight::SEMIBOLD
					} else {
						FontWeight::MEDIUM
					})
					.child(channel.name.clone()),
			)
			.when(channel.user_limit > 0, |row| {
				row.child(
					div()
						.flex_none()
						.text_size(theme.font_tiny)
						.child(format!("{}/{}", channel.occupants, channel.user_limit)),
				)
			})
			.when(channel.event_live, |row| {
				row.child(badge(cx.t("voice-live"), theme.danger, theme))
			})
			.children(chat_button)
			.children(plugins::channel_badges(id, &channel.badges, theme, cx))
			.when(channel.mentions > 0, |row| {
				row.child(MentionBadge::new(channel.mentions))
			})
			.when_some(dragged, |row, dragged| {
				row.on_drag(dragged, |dragged, _, _, cx| cx.new(|_| DragPreview::new(dragged)))
			})
			.children(drop_zone)
			.context_menu(menu);
		let topic = channel.topic.clone().map(|topic| {
			div()
				.truncate()
				.ml(theme.space_32 - theme.space_4)
				.mr(theme.space_8)
				.px(theme.space_8)
				.text_size(theme.font_xs)
				.text_color(theme.text_muted)
				.child(topic)
		});
		let spine = thread.then(|| thread_spine(channel.last_thread, theme));
		let status_line = channel.voice_status.clone().map(|status| {
			div()
				.min_w_0()
				.truncate()
				.ml(theme.space_32 - theme.space_4)
				.mr(theme.space_8)
				.px(theme.space_8)
				.pb(theme.space_4)
				.text_size(theme.font_tiny)
				.text_color(theme.text_muted)
				.child(status)
		});
		div()
			.relative()
			.flex()
			.flex_col()
			.children(spine)
			.child(row)
			.children(status_line)
			.children(topic)
			.children(participants)
			.into_any_element()
	}
}

fn thread_spine(last: bool, theme: &Theme) -> AnyElement {
	let line = theme.interactive_muted;
	let corner = theme.space_8;
	let x = theme.space_24 + theme.space_8;
	div()
		.absolute()
		.left(x)
		.top_0()
		.h(theme.channel_row_height)
		.w(theme.space_16)
		.child(
			div()
				.absolute()
				.left_0()
				.top_0()
				.w(theme.space_16)
				.h(theme.channel_row_height / 2.)
				.border_l_2()
				.border_b_2()
				.border_color(line)
				.rounded_bl(corner),
		)
		.when(!last, |spine| {
			spine.child(
				div()
					.absolute()
					.left_0()
					.top(theme.channel_row_height / 2.)
					.h(theme.channel_row_height / 2.)
					.border_l_2()
					.border_color(line),
			)
		})
		.into_any_element()
}

fn channel_item(row: &ChannelRow, guild: Snowflake, state: &state::State) -> ChannelItem {
	let voice_channel = matches!(row.kind, ChannelType::GuildVoice | ChannelType::GuildStageVoice);
	let permissions = state.my_permissions(row.id);
	ChannelItem {
		id: row.id,
		position: row.position,
		kind: row.kind,
		name: row.name.clone().into(),
		thread: is_thread(row.kind),
		last_thread: false,
		joined: row.member,
		icon: channel_icon(row.kind),
		voice_channel,
		event_live: voice_channel && state.event_live_in(guild, row.id),
		can_chat: voice_channel
			&& permissions.contains(Permissions::VIEW_CHANNEL | Permissions::READ_MESSAGE_HISTORY),
		can_invite: permissions.contains(Permissions::CREATE_INSTANT_INVITE),
		can_manage: permissions.contains(Permissions::MANAGE_CHANNELS),
		selectable: selectable(row.kind, row.locked),
		locked: row.locked,
		unread: row.unread,
		muted: row.muted,
		mentions: row.mentions,
		user_limit: row.user_limit,
		voice_status: row.voice_status.clone().map(Into::into),
		occupants: u32::try_from(row.voice_members.len()).unwrap_or(u32::MAX),
		join_block: row.join_block,
		voice: if voice_channel {
			participants(state, Some(guild), row.id, AVATAR_PIXELS)
		} else {
			Vec::new()
		},
		topic: state
			.stage_instance(guild, row.id)
			.map(|instance| instance.topic.clone().into()),
		badges: state
			.channel_decorations(row.id)
			.iter()
			.flat_map(|decoration| decoration.badges.iter().cloned())
			.collect(),
	}
}

fn drop_hidden(node: &mut ChannelTreeNode) -> bool {
	if node.category.as_ref().is_some_and(|category| category.locked) {
		return false;
	}
	let had_children = !node.children.is_empty();
	node.children.retain(|row| !row.locked);
	!had_children || !node.children.is_empty()
}

fn drop_muted(node: &mut ChannelTreeNode, selected: Option<Snowflake>) -> bool {
	let mut parent_kept = true;
	node.children.retain(|row| {
		if is_thread(row.kind) {
			return parent_kept;
		}
		parent_kept =
			!row.muted || row.mentions > 0 || Some(row.id) == selected || !row.voice_members.is_empty();
		parent_kept
	});
	!node.category.as_ref().is_some_and(|category| category.muted) || !node.children.is_empty()
}

fn category_item(node: &ChannelTreeNode, guild: Snowflake, state: &state::State) -> CategoryItem {
	CategoryItem {
		id: node.category.as_ref().map(|category| category.id),
		position: node.category.as_ref().map_or(0, |category| category.position),
		name: node
			.category
			.as_ref()
			.map(|category| category.name.clone().into())
			.unwrap_or_default(),
		collapsed: node.collapsed,
		can_create: node.category.as_ref().is_some_and(|category| {
			state
				.my_permissions(category.id)
				.contains(Permissions::MANAGE_CHANNELS)
		}),
		channels: with_thread_ends(
			node.children
				.iter()
				.map(|row| channel_item(row, guild, state))
				.collect(),
		),
	}
}

fn with_thread_ends(mut channels: Vec<ChannelItem>) -> Vec<ChannelItem> {
	let ends: Vec<bool> = channels
		.iter()
		.enumerate()
		.map(|(index, channel)| channel.thread && !channels.get(index + 1).is_some_and(|next| next.thread))
		.collect();
	for (channel, last) in channels.iter_mut().zip(ends) {
		channel.last_thread = last;
	}
	channels
}

impl Render for ChannelSidebar {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let categories: Vec<AnyElement> = self
			.categories
			.iter()
			.map(|category| self.category(category, &theme, cx))
			.collect();
		let events_row = self.events_row(&theme, cx);
		let top = plugins::mount_column("channel-list-top", cx);
		div()
			.flex()
			.flex_col()
			.size_full()
			.min_h_0()
			.bg(theme.background_tertiary)
			.child(self.header(&theme, cx))
			.when(!top.is_empty(), |sidebar| {
				sidebar.child(
					div()
						.flex()
						.flex_col()
						.flex_none()
						.gap(theme.space_4)
						.px(theme.space_8)
						.pt(theme.space_8)
						.children(top),
				)
			})
			.child(
				div()
					.flex()
					.flex_col()
					.pb(theme.space_16)
					.children(events_row)
					.children(categories)
					.overflow_y_scrollbar()
					.id("channel-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
