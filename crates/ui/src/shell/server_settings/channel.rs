use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, FontWeight,
	InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::scroll::ScrollableElement as _;
use protocol::{ChannelType, PermissionOverwriteKind, Permissions, Snowflake};
use session::Command;

use super::rights::GROUPS;
use crate::components::{
	ContextMenuBuilder, PickHandler, ToggleHandler, Tri, dialog_footer, field, hint, segmented, select,
	switch_row, tri_state,
};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind, channel_icon};
use crate::model::role_color;
use crate::shell::community::data_handle;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

type Named = Vec<(Snowflake, String)>;

const SLOWMODES: [(&str, u32); 14] = [
	("slowmode-off", 0),
	("slowmode-5s", 5),
	("slowmode-10s", 10),
	("slowmode-15s", 15),
	("slowmode-30s", 30),
	("slowmode-1m", 60),
	("slowmode-2m", 120),
	("slowmode-5m", 300),
	("slowmode-10m", 600),
	("slowmode-15m", 900),
	("slowmode-30m", 1800),
	("slowmode-1h", 3600),
	("slowmode-2h", 7200),
	("slowmode-6h", 21_600),
];
const KINDS: [(&str, ChannelType); 5] = [
	("channel-kind-text", ChannelType::GuildText),
	("channel-kind-voice", ChannelType::GuildVoice),
	("channel-kind-announcement", ChannelType::GuildAnnouncement),
	("channel-kind-forum", ChannelType::GuildForum),
	("channel-kind-stage", ChannelType::GuildStageVoice),
];
const NAME_LIMIT: usize = 100;
const TOPIC_LIMIT: usize = 1024;
const MEMBER_MENU_LIMIT: usize = 30;

const GENERAL: Permissions = Permissions::VIEW_CHANNEL
	.union(Permissions::MANAGE_CHANNELS)
	.union(Permissions::MANAGE_ROLES)
	.union(Permissions::MANAGE_WEBHOOKS)
	.union(Permissions::CREATE_INSTANT_INVITE);
const TEXT_ONLY: Permissions = Permissions::SEND_MESSAGES
	.union(Permissions::SEND_MESSAGES_IN_THREADS)
	.union(Permissions::CREATE_PUBLIC_THREADS)
	.union(Permissions::CREATE_PRIVATE_THREADS)
	.union(Permissions::EMBED_LINKS)
	.union(Permissions::ATTACH_FILES)
	.union(Permissions::ADD_REACTIONS)
	.union(Permissions::USE_EXTERNAL_EMOJIS)
	.union(Permissions::USE_EXTERNAL_STICKERS)
	.union(Permissions::MENTION_EVERYONE)
	.union(Permissions::MANAGE_MESSAGES)
	.union(Permissions::MANAGE_THREADS)
	.union(Permissions::READ_MESSAGE_HISTORY)
	.union(Permissions::SEND_TTS_MESSAGES)
	.union(Permissions::SEND_VOICE_MESSAGES)
	.union(Permissions::SEND_POLLS)
	.union(Permissions::USE_APPLICATION_COMMANDS)
	.union(Permissions::USE_EXTERNAL_APPS);
const VOICE_ONLY: Permissions = Permissions::CONNECT
	.union(Permissions::SPEAK)
	.union(Permissions::STREAM)
	.union(Permissions::USE_SOUNDBOARD)
	.union(Permissions::USE_EXTERNAL_SOUNDS)
	.union(Permissions::USE_VAD)
	.union(Permissions::PRIORITY_SPEAKER)
	.union(Permissions::MUTE_MEMBERS)
	.union(Permissions::DEAFEN_MEMBERS)
	.union(Permissions::MOVE_MEMBERS)
	.union(Permissions::USE_EMBEDDED_ACTIVITIES)
	.union(Permissions::CREATE_EVENTS)
	.union(Permissions::MANAGE_EVENTS)
	.union(Permissions::REQUEST_TO_SPEAK);

fn applicable(kind: ChannelType) -> Permissions {
	match kind {
		ChannelType::GuildVoice | ChannelType::GuildStageVoice => {
			GENERAL
				| VOICE_ONLY | Permissions::SEND_MESSAGES
				| Permissions::EMBED_LINKS
				| Permissions::ATTACH_FILES
				| Permissions::ADD_REACTIONS
				| Permissions::READ_MESSAGE_HISTORY
				| Permissions::USE_EXTERNAL_EMOJIS
				| Permissions::MANAGE_MESSAGES
		}
		ChannelType::GuildCategory => GENERAL | TEXT_ONLY | VOICE_ONLY,
		_ => GENERAL | TEXT_ONLY,
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
	Overview,
	Permissions,
}

#[derive(Clone)]
struct Overwrite {
	id: Snowflake,
	kind: PermissionOverwriteKind,
	allow: Permissions,
	deny: Permissions,
	saved: (Permissions, Permissions),
}

pub struct EditChannel {
	data: Entity<Data>,
	guild: Snowflake,
	channel: Snowflake,
	kind: ChannelType,
	tab: Tab,
	name: Entity<InputState>,
	topic: Entity<InputState>,
	slowmode: usize,
	nsfw: bool,
	parent: Option<Snowflake>,
	original_parent: Option<Snowflake>,
	original: (String, String, usize, bool),
	overwrites: Vec<Overwrite>,
	selected: Option<Snowflake>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for EditChannel {}

pub fn open_edit(
	window: &mut Window,
	cx: &mut App,
	guild: Snowflake,
	channel: Snowflake,
	permissions_tab: bool,
) {
	let data = data_handle(cx);
	let found = data.read(cx).state().channel(channel).cloned();
	let Some(found) = found else {
		return;
	};
	let view = cx.new(|cx| EditChannel::new(data, guild, &found, permissions_tab, window, cx));
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t_args(
		"channel-edit-title",
		&[("name", ArgValue::from(found.name.clone().unwrap_or_default()))],
	);
	let width = cx.global::<Theme>().modal_width_lg;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl EditChannel {
	fn new(
		data: Entity<Data>,
		guild: Snowflake,
		channel: &protocol::Channel,
		permissions_tab: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let name_value = channel.name.clone().unwrap_or_default();
		let topic_value = channel.topic.clone().unwrap_or_default();
		let slowmode_value = u32::try_from(channel.rate_limit_per_user.unwrap_or(0)).unwrap_or(0);
		let slowmode = SLOWMODES
			.iter()
			.position(|(_, seconds)| *seconds >= slowmode_value)
			.unwrap_or(0);
		let nsfw = channel.nsfw == Some(true);
		let topic_placeholder = cx.t("channel-topic-placeholder");
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(name_value.clone(), window, cx);
			state
		});
		let topic = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(topic_placeholder);
			state.set_value(topic_value.clone(), window, cx);
			state
		});
		let subscription = cx.subscribe_in(&name, window, |_, _, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		});
		let overwrites: Vec<Overwrite> = channel
			.permission_overwrites
			.iter()
			.map(|overwrite| Overwrite {
				id: overwrite.id,
				kind: overwrite.kind,
				allow: overwrite.allow,
				deny: overwrite.deny,
				saved: (overwrite.allow, overwrite.deny),
			})
			.collect();
		let selected = overwrites.first().map(|overwrite| overwrite.id);
		Self {
			data,
			guild,
			channel: channel.id,
			kind: channel.kind,
			tab: if permissions_tab {
				Tab::Permissions
			} else {
				Tab::Overview
			},
			name,
			topic,
			slowmode,
			nsfw,
			parent: channel.parent_id,
			original_parent: channel.parent_id,
			original: (name_value, topic_value, slowmode, nsfw),
			overwrites,
			selected,
			_subscription: subscription,
		}
	}

	fn text_like(&self) -> bool {
		matches!(
			self.kind,
			ChannelType::GuildText
				| ChannelType::GuildAnnouncement
				| ChannelType::GuildForum
				| ChannelType::GuildMedia
		)
	}

	fn save_overview(&mut self, cx: &mut Context<Self>) {
		let name: String = self
			.name
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(NAME_LIMIT)
			.collect();
		let topic: String = self
			.topic
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(TOPIC_LIMIT)
			.collect();
		let (old_name, old_topic, old_slowmode, old_nsfw) = &self.original;
		self.data.read(cx).send(Command::EditChannel {
			channel_id: self.channel,
			name: (!name.is_empty() && name != *old_name).then_some(name),
			topic: (self.text_like() && topic != *old_topic).then_some(topic),
			nsfw: (self.text_like() && self.nsfw != *old_nsfw).then_some(self.nsfw),
			slowmode: (self.text_like() && self.slowmode != *old_slowmode)
				.then(|| SLOWMODES[self.slowmode].1),
			position: None,
			parent_id: if self.parent != self.original_parent {
				self.parent
			} else {
				None
			},
		});
		cx.emit(Close);
	}

	fn save_permissions(&mut self, cx: &mut Context<Self>) {
		for overwrite in &mut self.overwrites {
			if (overwrite.allow, overwrite.deny) != overwrite.saved {
				self.data.read(cx).send(Command::SetChannelOverwrite {
					channel_id: self.channel,
					target_id: overwrite.id,
					kind: overwrite.kind,
					allow: overwrite.allow,
					deny: overwrite.deny,
				});
				overwrite.saved = (overwrite.allow, overwrite.deny);
			}
		}
		cx.notify();
	}

	fn permissions_dirty(&self) -> bool {
		self.overwrites
			.iter()
			.any(|overwrite| (overwrite.allow, overwrite.deny) != overwrite.saved)
	}

	fn overview_dirty(&self, cx: &App) -> bool {
		let (old_name, old_topic, old_slowmode, old_nsfw) = &self.original;
		let name = self.name.read(cx).value();
		let topic = self.topic.read(cx).value();
		(name.trim() != old_name && !name.trim().is_empty())
			|| topic.trim() != old_topic
			|| self.slowmode != *old_slowmode
			|| self.nsfw != *old_nsfw
			|| self.parent != self.original_parent
	}

	fn add_overwrite(&mut self, id: Snowflake, kind: PermissionOverwriteKind, cx: &mut Context<Self>) {
		if self.overwrites.iter().any(|overwrite| overwrite.id == id) {
			self.selected = Some(id);
			cx.notify();
			return;
		}
		self.data.read(cx).send(Command::SetChannelOverwrite {
			channel_id: self.channel,
			target_id: id,
			kind,
			allow: Permissions::empty(),
			deny: Permissions::empty(),
		});
		self.overwrites.push(Overwrite {
			id,
			kind,
			allow: Permissions::empty(),
			deny: Permissions::empty(),
			saved: (Permissions::empty(), Permissions::empty()),
		});
		self.selected = Some(id);
		cx.notify();
	}

	fn remove_overwrite(&mut self, id: Snowflake, cx: &mut Context<Self>) {
		self.data.read(cx).send(Command::DeleteChannelOverwrite {
			channel_id: self.channel,
			target_id: id,
		});
		self.overwrites.retain(|overwrite| overwrite.id != id);
		self.selected = self.overwrites.first().map(|overwrite| overwrite.id);
		cx.notify();
	}

	fn target_name(&self, overwrite: &Overwrite, cx: &App) -> (String, Option<gpui::Hsla>) {
		let state = self.data.read(cx).state();
		match overwrite.kind {
			PermissionOverwriteKind::Member => (state.display_name(Some(self.guild), overwrite.id), None),
			_ => state
				.guild(self.guild)
				.and_then(|guild| guild.roles.get(&overwrite.id))
				.map_or_else(
					|| (overwrite.id.to_string(), None),
					|role| {
						let name = if role.id == self.guild {
							"@everyone".to_owned()
						} else {
							role.name.clone()
						};
						(
							name,
							u32::try_from(role.color)
								.ok()
								.filter(|color| *color != 0)
								.map(role_color),
						)
					},
				),
		}
	}

	fn add_menu(&self, cx: &mut Context<Self>) -> ContextMenuBuilder {
		let (roles, members): (Named, Named) = {
			let state = self.data.read(cx).state();
			let Some(guild) = state.guild(self.guild) else {
				return ContextMenuBuilder::new();
			};
			let mut roles: Vec<_> = guild
				.roles
				.values()
				.filter(|role| !self.overwrites.iter().any(|entry| entry.id == role.id))
				.map(|role| (role.position, role.id, role.name.clone()))
				.collect();
			roles.sort_by_key(|(position, _, _)| std::cmp::Reverse(*position));
			let mut members: Vec<(Snowflake, String)> = guild
				.members
				.keys()
				.filter(|id| !self.overwrites.iter().any(|entry| entry.id == **id))
				.map(|id| (*id, state.display_name(Some(self.guild), *id)))
				.collect();
			members.sort_by_key(|(_, name)| name.to_lowercase());
			members.truncate(MEMBER_MENU_LIMIT);
			(
				roles
					.into_iter()
					.map(|(_, id, name)| {
						(
							id,
							if id == self.guild {
								"@everyone".to_owned()
							} else {
								name
							},
						)
					})
					.collect(),
				members,
			)
		};
		let weak = cx.weak_entity();
		let mut role_menu = ContextMenuBuilder::new();
		for (id, name) in roles {
			let weak = weak.clone();
			role_menu = role_menu.item(name, None, move |_, cx| {
				weak.update(cx, |this, cx| {
					this.add_overwrite(id, PermissionOverwriteKind::Role, cx)
				})
				.ok();
			});
		}
		let mut member_menu = ContextMenuBuilder::new();
		for (id, name) in members {
			let weak = weak.clone();
			member_menu = member_menu.item(name, None, move |_, cx| {
				weak.update(cx, |this, cx| {
					this.add_overwrite(id, PermissionOverwriteKind::Member, cx)
				})
				.ok();
			});
		}
		let mut menu = ContextMenuBuilder::new();
		if !role_menu.is_empty() {
			menu = menu.submenu(cx.t("channel-perm-add-role"), Some(IconKind::Shield), role_menu);
		}
		if !member_menu.is_empty() {
			menu = menu.submenu(
				cx.t("channel-perm-add-member"),
				Some(IconKind::Members),
				member_menu,
			);
		}
		menu
	}

	fn overview(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let weak = cx.weak_entity();
		let slow_labels: Vec<SharedString> = SLOWMODES.iter().map(|(key, _)| cx.t(key)).collect();
		let pick_slow: PickHandler = {
			let weak = weak.clone();
			Rc::new(move |index, _, cx| {
				weak.update(cx, |this, cx| {
					this.slowmode = index;
					cx.notify();
				})
				.ok();
			})
		};
		let nsfw: ToggleHandler = {
			let weak = weak.clone();
			Rc::new(move |value, _, cx| {
				weak.update(cx, |this, cx| {
					this.nsfw = value;
					cx.notify();
				})
				.ok();
			})
		};
		let categories: Vec<(Snowflake, String)> = {
			let state = self.data.read(cx).state();
			state
				.guild(self.guild)
				.map(|guild| {
					guild
						.channels
						.values()
						.filter(|channel| channel.kind == ChannelType::GuildCategory)
						.map(|channel| (channel.id, channel.name.clone().unwrap_or_default()))
						.collect()
				})
				.unwrap_or_default()
		};
		let mut labels: Vec<SharedString> = Vec::new();
		let mut ids: Vec<Option<Snowflake>> = Vec::new();
		if self.original_parent.is_none() {
			labels.push(cx.t("channel-no-category"));
			ids.push(None);
		}
		for (id, name) in categories {
			labels.push(name.into());
			ids.push(Some(id));
		}
		let selected_parent = ids.iter().position(|id| *id == self.parent).unwrap_or(0);
		let pick_parent: PickHandler = {
			let ids = ids.clone();
			Rc::new(move |index, _, cx| {
				if let Some(parent) = ids.get(index).copied().flatten() {
					weak.update(cx, |this, cx| {
						this.parent = Some(parent);
						cx.notify();
					})
					.ok();
				}
			})
		};
		let text = self.text_like();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_20)
			.child(field(cx.t("channel-name"), theme, Input::new(&self.name)))
			.when(text, |form| {
				form.child(field(cx.t("channel-topic"), theme, Input::new(&self.topic)))
					.child(field(
						cx.t("channel-slowmode"),
						theme,
						select(
							"channel-slowmode",
							slow_labels[self.slowmode].clone(),
							&slow_labels,
							self.slowmode,
							pick_slow,
						),
					))
					.child(switch_row(
						"channel-nsfw",
						cx.t("channel-nsfw"),
						Some(cx.t("channel-nsfw-hint")),
						self.nsfw,
						theme,
						nsfw,
					))
			})
			.when(
				!labels.is_empty() && self.kind != ChannelType::GuildCategory,
				|form| {
					form.child(field(
						cx.t("channel-category"),
						theme,
						select(
							"channel-category",
							labels[selected_parent].clone(),
							&labels,
							selected_parent,
							pick_parent,
						),
					))
				},
			)
			.into_any_element()
	}

	fn permissions(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let mask = applicable(self.kind);
		let list: Vec<AnyElement> = self
			.overwrites
			.iter()
			.map(|overwrite| {
				let id = overwrite.id;
				let (name, color) = self.target_name(overwrite, cx);
				let active = self.selected == Some(id);
				let member = overwrite.kind == PermissionOverwriteKind::Member;
				div()
					.id(("overwrite", id.get()))
					.flex()
					.items_center()
					.gap(theme.space_8)
					.h(theme.control_height)
					.px(theme.space_8)
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.when(active, |row| row.bg(theme.modifier_selected))
					.when(!active, |row| row.hover(|style| style.bg(theme.modifier_hover)))
					.on_click(cx.listener(move |this, _, _, cx| {
						this.selected = Some(id);
						cx.notify();
					}))
					.child(
						Icon::new(if member {
							IconKind::Account
						} else {
							IconKind::Shield
						})
						.size(theme.glyph_sm)
						.color(color.unwrap_or(theme.interactive_normal)),
					)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.text_size(theme.font_sm)
							.font_weight(FontWeight::MEDIUM)
							.text_color(theme.header_primary)
							.child(name),
					)
					.into_any_element()
			})
			.collect();
		let add_menu = self.add_menu(cx);
		let detail: Option<AnyElement> = self
			.selected
			.and_then(|id| {
				self.overwrites
					.iter()
					.find(|overwrite| overwrite.id == id)
					.cloned()
			})
			.map(|overwrite| {
				let id = overwrite.id;
				let rows: Vec<AnyElement> = GROUPS
					.iter()
					.flat_map(|group| group.rights.iter())
					.filter(|right| mask.contains(right.flag))
					.map(|right| {
						let flag = right.flag;
						let value = if overwrite.allow.contains(flag) {
							Tri::Allow
						} else if overwrite.deny.contains(flag) {
							Tri::Deny
						} else {
							Tri::Neutral
						};
						let weak = cx.weak_entity();
						div()
							.flex()
							.items_center()
							.justify_between()
							.gap(theme.space_12)
							.py(theme.space_8)
							.border_b_1()
							.border_color(theme.divider)
							.child(
								div()
									.flex()
									.flex_col()
									.min_w_0()
									.child(
										div()
											.text_size(theme.font_md)
											.font_weight(FontWeight::MEDIUM)
											.text_color(theme.header_primary)
											.child(cx.t(right.name)),
									)
									.child(hint(cx.t(right.hint), theme)),
							)
							.child(tri_state(
								("overwrite-right", flag.bits() as usize),
								value,
								theme,
								Rc::new(move |tri, _, cx| {
									weak.update(cx, |this, cx| {
										if let Some(entry) =
											this.overwrites.iter_mut().find(|entry| entry.id == id)
										{
											entry.allow.remove(flag);
											entry.deny.remove(flag);
											match tri {
												Tri::Allow => entry.allow.insert(flag),
												Tri::Deny => entry.deny.insert(flag),
												Tri::Neutral => {}
											}
										}
										cx.notify();
									})
									.ok();
								}),
							))
							.into_any_element()
					})
					.collect();
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.pl(theme.space_16)
					.child(
						div().flex().flex_none().justify_end().pb(theme.space_8).child(
							Button::new("overwrite-remove")
								.danger()
								.outline()
								.label(cx.t("channel-perm-remove"))
								.on_click(cx.listener(move |this, _, _, cx| this.remove_overwrite(id, cx))),
						),
					)
					.child(
						div()
							.flex()
							.flex_col()
							.children(rows)
							.overflow_y_scrollbar()
							.id("overwrite-rights")
							.flex_1()
							.min_h_0(),
					)
					.into_any_element()
			});
		div()
			.flex()
			.h(theme.popover_height)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_none()
					.w(theme.manage_grid_cell * 2.)
					.gap(theme.space_8)
					.child(
						Button::new("overwrite-add")
							.outline()
							.icon(gpui_component::Icon::default().path(IconKind::Plus.path()))
							.label(cx.t("channel-perm-add"))
							.dropdown_menu(add_menu.build()),
					)
					.child(
						div()
							.flex()
							.flex_col()
							.gap(theme.space_2)
							.children(list)
							.overflow_y_scrollbar()
							.id("overwrite-list")
							.flex_1()
							.min_h_0(),
					),
			)
			.children(detail)
			.into_any_element()
	}
}

impl Render for EditChannel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let labels = [cx.t("channel-tab-overview"), cx.t("channel-tab-permissions")];
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.tab = if index == 0 {
					Tab::Overview
				} else {
					Tab::Permissions
				};
				cx.notify();
			})
			.ok();
		});
		let body = match self.tab {
			Tab::Overview => self.overview(&theme, cx),
			Tab::Permissions => self.permissions(&theme, cx),
		};
		let (enabled, confirm_key) = match self.tab {
			Tab::Overview => (self.overview_dirty(cx), "settings-save"),
			Tab::Permissions => (self.permissions_dirty(), "settings-save"),
		};
		let weak_cancel = cx.weak_entity();
		let weak_save = cx.weak_entity();
		let tab = self.tab;
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(segmented(
				"channel-tabs",
				&labels,
				usize::from(self.tab == Tab::Permissions),
				&theme,
				pick,
			))
			.child(body)
			.child(dialog_footer(
				cx.t("action-close"),
				cx.t(confirm_key),
				false,
				enabled,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_save
						.update(cx, |this, cx| match tab {
							Tab::Overview => this.save_overview(cx),
							Tab::Permissions => this.save_permissions(cx),
						})
						.ok();
				},
			))
	}
}

pub struct CreateChannel {
	data: Entity<Data>,
	guild: Snowflake,
	parent: Option<Snowflake>,
	kind: usize,
	name: Entity<InputState>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for CreateChannel {}

pub fn open_create(window: &mut Window, cx: &mut App, guild: Snowflake, parent: Option<Snowflake>) {
	let data = data_handle(cx);
	let placeholder = cx.t("channel-create-name-placeholder");
	let view = cx.new(|cx| {
		let name = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscription = cx.subscribe_in(
			&name,
			window,
			|this: &mut CreateChannel, _, event: &InputEvent, _, cx| match event {
				InputEvent::Change => cx.notify(),
				InputEvent::PressEnter { .. } => this.create(cx),
				_ => {}
			},
		);
		CreateChannel {
			data,
			guild,
			parent,
			kind: 0,
			name,
			_subscription: subscription,
		}
	});
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t("channel-create-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

fn normalize(name: &str, kind: ChannelType) -> String {
	let trimmed = name.trim();
	if matches!(kind, ChannelType::GuildVoice | ChannelType::GuildStageVoice) {
		return trimmed.chars().take(NAME_LIMIT).collect();
	}
	trimmed
		.to_lowercase()
		.split_whitespace()
		.collect::<Vec<_>>()
		.join("-")
		.chars()
		.take(NAME_LIMIT)
		.collect()
}

impl CreateChannel {
	fn create(&mut self, cx: &mut Context<Self>) {
		let kind = KINDS[self.kind].1;
		let name = normalize(&self.name.read(cx).value(), kind);
		if name.is_empty() {
			return;
		}
		self.data.read(cx).send(Command::CreateChannel {
			guild_id: self.guild,
			name,
			kind,
			parent_id: self.parent,
		});
		cx.emit(Close);
	}
}

impl Render for CreateChannel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let labels: Vec<SharedString> = KINDS.iter().map(|(key, _)| cx.t(key)).collect();
		let kind = KINDS[self.kind].1;
		let enabled = !normalize(&self.name.read(cx).value(), kind).is_empty();
		let weak_cancel = cx.weak_entity();
		let weak_create = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(
				cx.t("channel-kind"),
				&theme,
				div()
					.flex()
					.flex_wrap()
					.gap(theme.space_8)
					.children(KINDS.iter().enumerate().map(|(index, (_, kind))| {
						let active = index == self.kind;
						let weak = cx.weak_entity();
						div()
							.id(("channel-kind", index))
							.flex()
							.items_center()
							.gap(theme.space_8)
							.h(theme.control_height)
							.px(theme.space_12)
							.rounded(theme.radius_sm)
							.cursor_pointer()
							.border_1()
							.border_color(if active { theme.brand } else { theme.divider })
							.bg(if active {
								theme.modifier_selected
							} else {
								theme.background_secondary
							})
							.on_click(move |_, _, cx| {
								weak.update(cx, |this, cx| {
									this.kind = index;
									cx.notify();
								})
								.ok();
							})
							.child(
								Icon::new(channel_icon(*kind))
									.size(theme.glyph_sm)
									.color(theme.interactive_normal),
							)
							.child(
								div()
									.text_size(theme.font_sm)
									.font_weight(FontWeight::MEDIUM)
									.text_color(theme.header_primary)
									.child(labels[index].clone()),
							)
					})),
			))
			.child(field(cx.t("channel-name"), &theme, Input::new(&self.name)))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("channel-create-confirm"),
				false,
				enabled,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_create.update(cx, |this, cx| this.create(cx)).ok();
				},
			))
	}
}

pub fn confirm_delete(window: &mut Window, cx: &mut App, channel: Snowflake, name: String) {
	let data = data_handle(cx);
	window.open_alert_dialog(cx, move |alert, _, cx| {
		let data = data.clone();
		alert
			.title(cx.t_args("channel-delete-title", &[("name", ArgValue::from(name.clone()))]))
			.description(cx.t("channel-delete-body"))
			.confirm()
			.ok_text(cx.t("channel-delete"))
			.ok_variant(ButtonVariant::Danger)
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				data.read(cx).send(Command::DeleteChannel { channel_id: channel });
				true
			})
	});
}
