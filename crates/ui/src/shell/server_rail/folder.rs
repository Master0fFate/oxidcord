use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::Snowflake;
use session::Command;
use state::RailEntry;

use super::reorder::{Drop, new_folder_id, rearranged, restyled, ungrouped};
use super::{DEFAULT_FOLDER_COLOR, Id, folder_title};
use crate::components::{
	COLOR_PALETTE, ContextMenuBuilder, color_swatch, dialog_footer, field, parse_hex_color,
};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::shell::community::data_handle;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const NAME_LIMIT: usize = 32;

fn send_layout(cx: &App, change: impl FnOnce(&[RailEntry]) -> Option<Vec<RailEntry>>) {
	let data = data_handle(cx);
	let data = data.read(cx);
	let layout = data.state().guild_layout();
	if let Some(next) = change(&layout) {
		data.send(Command::SetGuildLayout(next));
	}
}

pub(super) fn folder_menu(id: Id) -> ContextMenuBuilder {
	ContextMenuBuilder::new().lazy(move |cx| folder_menu_now(id, cx))
}

fn folder_menu_now(id: Id, cx: &App) -> ContextMenuBuilder {
	let data = data_handle(cx);
	let (guilds, unread) = {
		let state = data.read(cx).state();
		let guilds: Vec<Snowflake> = state
			.guild_layout()
			.into_iter()
			.find_map(|entry| match entry {
				RailEntry::Folder {
					id: folder, guilds, ..
				} if folder == id => Some(guilds),
				_ => None,
			})
			.unwrap_or_default();
		let unread = guilds
			.iter()
			.filter_map(|guild| state.guild_summary(*guild))
			.any(|summary| summary.unread || summary.mentions > 0);
		(guilds, unread)
	};
	let read_label = cx.t("folder-mark-read");
	let menu = if unread {
		ContextMenuBuilder::new().item(read_label, Some(IconKind::Check), move |_, cx| {
			let data = data_handle(cx);
			for guild_id in &guilds {
				data.read(cx).send(Command::MarkGuildRead { guild_id: *guild_id });
			}
		})
	} else {
		ContextMenuBuilder::new().disabled_item(read_label, Some(IconKind::Check))
	};
	menu.item(
		cx.t("folder-settings"),
		Some(IconKind::Settings),
		move |window, cx| open(window, cx, id),
	)
	.separator()
	.item(cx.t("folder-ungroup"), Some(IconKind::Folder), move |_, cx| {
		send_layout(cx, |layout| ungrouped(layout, id));
	})
}

pub(super) fn move_menu(guild: Snowflake, cx: &App) -> ContextMenuBuilder {
	let data = data_handle(cx);
	let state = data.read(cx).state();
	let layout = state.guild_layout();
	let mut destinations = ContextMenuBuilder::new();
	let mut current = None;
	for entry in &layout {
		let RailEntry::Folder { id, name, guilds, .. } = entry else {
			continue;
		};
		let member = guilds.contains(&guild);
		if member {
			current = Some(*id);
		}
		let title = folder_title(
			name.clone(),
			guilds
				.iter()
				.filter_map(|id| state.guild(*id))
				.map(|guild| guild.name.as_str()),
		);
		let id = *id;
		destinations = destinations.checked_item(title, member, move |_, cx| {
			if !member {
				send_layout(cx, |layout| {
					rearranged(layout, guild, Drop::OntoFolder(id), new_folder_id())
				});
			}
		});
	}
	drop(state);
	if destinations.is_empty() {
		return destinations;
	}
	if current.is_some() {
		destinations = destinations
			.separator()
			.item(cx.t("folder-none"), None, move |_, cx| {
				send_layout(cx, |layout| rearranged(layout, guild, Drop::End, new_folder_id()));
			});
	}
	ContextMenuBuilder::new()
		.separator()
		.submenu(cx.t("folder-move"), Some(IconKind::Folder), destinations)
}

pub struct FolderSettings {
	data: Entity<Data>,
	id: Id,
	color: u32,
	name: Entity<InputState>,
	hex: Entity<InputState>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<Close> for FolderSettings {}

pub(super) fn open(window: &mut Window, cx: &mut App, id: Id) {
	let data = data_handle(cx);
	let current = data
		.read(cx)
		.state()
		.guild_layout()
		.into_iter()
		.find_map(|entry| match entry {
			RailEntry::Folder {
				id: folder,
				name,
				color,
				..
			} if folder == id => Some((name, color)),
			_ => None,
		});
	let Some((name, color)) = current else {
		return;
	};
	let color = color.unwrap_or(DEFAULT_FOLDER_COLOR);
	let view = cx.new(|cx| FolderSettings::new(data, id, name.unwrap_or_default(), color, window, cx));
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t("folder-settings");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl FolderSettings {
	fn new(
		data: Entity<Data>,
		id: Id,
		name: String,
		color: u32,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let placeholder = cx.t("folder-name");
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(placeholder);
			state.set_value(name, window, cx);
			state
		});
		let hex = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder("#5865f2");
			state.set_value(format!("#{color:06x}"), window, cx);
			state
		});
		let subscriptions = vec![
			cx.subscribe_in(&name, window, |this, _, event: &InputEvent, _, cx| match event {
				InputEvent::PressEnter { .. } => this.save(cx),
				InputEvent::Change => cx.notify(),
				_ => {}
			}),
			cx.subscribe_in(&hex, window, |this, input, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change)
					&& let Some(color) = parse_hex_color(&input.read(cx).value())
				{
					this.color = color;
					cx.notify();
				}
			}),
		];
		Self {
			data,
			id,
			color,
			name,
			hex,
			_subscriptions: subscriptions,
		}
	}

	fn save(&mut self, cx: &mut Context<Self>) {
		let name: String = self
			.name
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(NAME_LIMIT)
			.collect();
		let name = (!name.is_empty()).then_some(name);
		let data = self.data.read(cx);
		let layout = data.state().guild_layout();
		if let Some(next) = restyled(&layout, self.id, name, Some(self.color)) {
			data.send(Command::SetGuildLayout(next));
		}
		cx.emit(Close);
	}
}

impl Render for FolderSettings {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let swatches: Vec<_> = COLOR_PALETTE
			.iter()
			.map(|color| {
				let color = *color;
				color_swatch(
					color,
					self.color == color,
					&theme,
					cx.listener(move |this, _, window, cx| {
						this.color = color;
						this.hex.update(cx, |state, cx| {
							state.set_value(format!("#{color:06x}"), window, cx)
						});
						cx.notify();
					}),
				)
			})
			.collect();
		let weak = cx.weak_entity();
		let cancel = weak.clone();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(cx.t("folder-name"), &theme, Input::new(&self.name)))
			.child(field(
				cx.t("folder-color"),
				&theme,
				div()
					.flex()
					.flex_col()
					.gap(theme.space_12)
					.child(div().flex().flex_wrap().gap(theme.space_8).children(swatches))
					.child(Input::new(&self.hex)),
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("folder-save"),
				false,
				true,
				&theme,
				move |_, cx| {
					cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak.update(cx, |this, cx| this.save(cx)).ok();
				},
			))
	}
}
