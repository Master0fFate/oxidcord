use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::{
	Animation, AnimationExt as _, AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight,
	Hsla, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _, Rems, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, div, ease_out_quint, img,
	prelude::FluentBuilder as _, px, transparent_black,
};
use gpui_component::menu::ContextMenuExt as _;
use media::SizeHint;
use protocol::Snowflake;
use session::Command;
use state::State;

use super::community::add_server;
use super::community::menus::guild_menu_for;
use super::plugins::{self, MenuTarget, PluginMenu as _};
use super::transition::Pairs;
use crate::components::{Divider, MentionBadge, PillState, UnreadPill, tooltip};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::images::Images;
use crate::model::role_color;
use crate::theme::{Theme, rem, with_alpha};

use reorder::{DragPreview, DraggedGuild, Drop, new_folder_id, rearranged};

mod reorder;

type Id = u64;

const ICON_PIXELS: u32 = 96;
const DEFAULT_FOLDER_COLOR: u32 = 0x5865f2;
const FOLDER_NAME_GUILDS: usize = 3;
const SERVER_DIRECTORY: &str = "https://discord.com/servers";

#[derive(Clone)]
pub struct GuildItem {
	pub id: Snowflake,
	pub name: SharedString,
	pub icon: Option<String>,
	pub unread: bool,
	pub mentions: u32,
}

#[derive(Clone)]
pub struct FolderItem {
	pub id: Id,
	pub name: SharedString,
	pub color: Hsla,
	pub guilds: Vec<GuildItem>,
}

#[derive(Clone)]
pub enum RailEntry {
	Guild(GuildItem),
	Folder(FolderItem),
}

const MORPH: Duration = Duration::from_millis(140);

pub enum RailEvent {
	SelectHome,
	SelectGuild(Snowflake),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Target {
	Home,
	Guild(Id),
	Folder(Id),
	AddServer,
	Explore,
}

impl Target {
	fn seed(self) -> u64 {
		match self {
			Self::Home => 1,
			Self::AddServer => 2,
			Self::Explore => 3,
			Self::Guild(id) => id.wrapping_mul(2).wrapping_add(8),
			Self::Folder(id) => id.wrapping_mul(2).wrapping_add(9),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TileState {
	pill: PillState,
	round: bool,
}

struct MiniGuild {
	icon: Option<String>,
	letter: SharedString,
}

enum Face {
	Icon(IconKind),
	Guild {
		icon: Option<String>,
		acronym: SharedString,
	},
	Folder(Vec<MiniGuild>),
}

struct TileStyle {
	rest_background: Hsla,
	rest_foreground: Hsla,
	active_background: Hsla,
	active_foreground: Hsla,
}

struct TileSpec {
	target: Target,
	face: Face,
	style: TileStyle,
	unread: bool,
	selected: bool,
	mentions: u32,
	pill_inset: Rems,
}

pub struct ServerRail {
	data: Entity<Data>,
	entries: Vec<RailEntry>,
	selected: Option<Snowflake>,
	home_mentions: u32,
	hovered: Option<Target>,
	open_folders: HashSet<Id>,
	tiles: Pairs<Target, TileState>,
}

impl EventEmitter<RailEvent> for ServerRail {}

impl ServerRail {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let mut rail = Self {
			data,
			entries: Vec::new(),
			selected: None,
			home_mentions: 0,
			hovered: None,
			open_folders: HashSet::new(),
			tiles: Pairs::default(),
		};
		rail.refresh(cx);
		rail
	}

	pub fn select(&mut self, guild: Option<Snowflake>, cx: &mut Context<Self>) {
		if self.selected != guild {
			self.selected = guild;
			if let Some(guild) = guild {
				self.open_folder_of(guild);
			}
			cx.notify();
		}
	}

	fn open_folder_of(&mut self, guild: Snowflake) {
		let folder = self.entries.iter().find_map(|entry| match entry {
			RailEntry::Folder(folder) if folder.guilds.iter().any(|item| item.id == guild) => Some(folder.id),
			_ => None,
		});
		if let Some(id) = folder {
			self.open_folders.insert(id);
		}
	}

	fn drop_guild(&mut self, guild: Snowflake, drop: Drop, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let layout = data.state().guild_layout();
		if let Some(next) = rearranged(&layout, guild, drop, new_folder_id()) {
			data.send(Command::SetGuildLayout(next));
		}
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let state = data.state();
		self.entries = rail_entries(&state);
		self.home_mentions = state.dm_rows().iter().map(|row| row.mentions).sum();
		drop(state);
		cx.notify();
	}

	fn set_hover(&mut self, target: Target, hovered: bool, cx: &mut Context<Self>) {
		let next = if hovered {
			Some(target)
		} else if self.hovered == Some(target) {
			None
		} else {
			self.hovered
		};
		if next != self.hovered {
			self.hovered = next;
			cx.notify();
		}
	}

	fn toggle_folder(&mut self, id: Id, cx: &mut Context<Self>) {
		if !self.open_folders.remove(&id) {
			self.open_folders.insert(id);
		}
		cx.notify();
	}

	fn tile(&mut self, spec: TileSpec, theme: &Theme, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
		let TileSpec {
			target,
			face,
			style,
			unread: pill_unread,
			selected,
			mentions,
			pill_inset,
		} = spec;
		let hovered = self.hovered == Some(target);
		let pill = if selected {
			PillState::Selected
		} else if hovered {
			PillState::Hovered
		} else if pill_unread {
			PillState::Unread
		} else {
			PillState::Hidden
		};
		let state = TileState {
			pill,
			round: selected || hovered,
		};
		let (from, to) = self.tiles.settle(target, state);
		let seed = target.seed();
		let radius = |round: bool| if round { 16. } else { 24. };
		let (from_radius, to_radius) = (radius(from.round), radius(to.round));
		let animation_key = seed
			.wrapping_mul(16)
			.wrapping_add((to.pill as u64) * 2 + u64::from(to.round));
		let (background, foreground) = if to.round {
			(style.active_background, style.active_foreground)
		} else {
			(style.rest_background, style.rest_foreground)
		};
		let size = theme.server_icon;
		let ring = theme.background_tertiary;
		let space_4 = theme.space_4;
		let face = match face {
			Face::Icon(kind) => Icon::new(kind)
				.size(rem(26.))
				.color(foreground)
				.into_any_element(),
			Face::Guild { icon, acronym } => {
				let image = icon
					.as_deref()
					.and_then(|url| cx.global::<Images>().get(url, SizeHint::square(ICON_PIXELS)));
				match image {
					Some(image) => img(image)
						.size_full()
						.object_fit(ObjectFit::Cover)
						.with_animation(
							("rail-face", animation_key),
							Animation::new(MORPH).with_easing(ease_out_quint()),
							move |face, delta| {
								face.rounded(rem(from_radius + (to_radius - from_radius) * delta))
							},
						)
						.into_any_element(),
					None => {
						let font = match acronym.chars().count() {
							0..=2 => theme.font_lg * 0.9,
							3 => theme.font_md,
							_ => theme.font_sm,
						};
						div()
							.text_size(font)
							.font_weight(FontWeight::MEDIUM)
							.text_color(foreground)
							.child(acronym)
							.into_any_element()
					}
				}
			}
			Face::Folder(letters) => {
				let cell = theme.space_16;
				div()
					.flex()
					.flex_wrap()
					.size_full()
					.p(rem(6.))
					.gap(space_4)
					.children(letters.into_iter().map(|mini| {
						let image = mini
							.icon
							.as_deref()
							.and_then(|url| cx.global::<Images>().get(url, SizeHint::square(ICON_PIXELS)));
						div()
							.flex()
							.items_center()
							.justify_center()
							.size(cell)
							.rounded_full()
							.overflow_hidden()
							.bg(theme.background_primary)
							.text_size(rem(9.))
							.text_color(theme.text_normal)
							.map(|cell| match image {
								Some(image) => {
									cell.child(img(image).size_full().object_fit(ObjectFit::Cover))
								}
								None => cell.child(mini.letter),
							})
					}))
					.into_any_element()
			}
		};
		let content = div()
			.absolute()
			.top_0()
			.left_0()
			.size(size)
			.flex()
			.items_center()
			.justify_center()
			.overflow_hidden()
			.bg(background)
			.child(face)
			.with_animation(
				("rail-morph", animation_key),
				Animation::new(MORPH).with_easing(ease_out_quint()),
				move |tile, delta| tile.rounded(rem(from_radius + (to_radius - from_radius) * delta)),
			);
		div()
			.id(("rail-tile", seed))
			.relative()
			.flex_none()
			.w_full()
			.h(size)
			.flex()
			.justify_center()
			.cursor_pointer()
			.on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.set_hover(target, *hovered, cx)))
			.child(UnreadPill::new(("rail-pill", seed), from.pill, to.pill).inset(pill_inset))
			.child(
				div()
					.relative()
					.flex_none()
					.size(size)
					.child(content)
					.when(mentions > 0, |icon| {
						icon.child(
							div()
								.absolute()
								.right(rem(-6.))
								.bottom(rem(-6.))
								.child(MentionBadge::new(mentions).ring(ring)),
						)
					}),
			)
	}
}

fn acronym(name: &str) -> SharedString {
	name.split_whitespace()
		.filter_map(|word| word.chars().next())
		.take(5)
		.flat_map(char::to_uppercase)
		.collect::<String>()
		.into()
}

impl ServerRail {
	fn guild_tile(
		&mut self,
		guild: &GuildItem,
		nested: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let id = guild.id;
		let selected = self.selected == Some(id);
		let face = Face::Guild {
			icon: guild.icon.clone(),
			acronym: acronym(&guild.name),
		};
		let style = TileStyle {
			rest_background: theme.background_primary,
			rest_foreground: theme.text_normal,
			active_background: theme.brand,
			active_foreground: theme.text_on_brand,
		};
		let name = guild.name.clone();
		let dragged = DraggedGuild {
			id,
			icon: guild.icon.clone(),
			acronym: acronym(&guild.name),
		};
		let menu = guild_menu_for(id)
			.plugin_items("server-context-menu", MenuTarget::server(id), cx)
			.build();
		let target = Target::Guild(id.get());
		let tile = self
			.tile(
				TileSpec {
					target,
					face,
					style,
					unread: guild.unread,
					selected,
					mentions: guild.mentions,
					pill_inset: if nested {
						(theme.server_icon - theme.server_rail_width) / 2.
					} else {
						rem(0.)
					},
				},
				theme,
				cx,
			)
			.on_click(cx.listener(move |_, _, _, cx| cx.emit(RailEvent::SelectGuild(id))))
			.tooltip(tooltip(name))
			.on_drag(dragged, |dragged, _, _, cx| cx.new(|_| DragPreview::new(dragged)))
			.context_menu(menu);
		let inset = if nested {
			rem(0.)
		} else {
			(theme.server_rail_width - theme.server_icon) / 2.
		};
		self.drop_slot(
			tile,
			target,
			(Drop::OntoGuild(id), Drop::BeforeGuild(id)),
			inset,
			theme,
			cx,
		)
	}

	fn drop_slot(
		&self,
		tile: impl IntoElement,
		target: Target,
		(onto, before): (Drop, Drop),
		inset: Rems,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let brand = theme.brand;
		let seed = target.seed();
		div()
			.id(("rail-drop", seed))
			.relative()
			.flex_none()
			.w_full()
			.child(tile)
			.child(
				div()
					.id(("rail-onto", seed))
					.absolute()
					.top_0()
					.left(inset)
					.size(theme.server_icon)
					.rounded(theme.radius_lg)
					.border_2()
					.border_color(transparent_black())
					.drag_over::<DraggedGuild>(move |style, _, _, _| style.border_color(brand))
					.on_drop(cx.listener(move |this, dragged: &DraggedGuild, _, cx| {
						this.drop_guild(dragged.id, onto, cx)
					})),
			)
			.child(self.gap_zone(seed, before, inset, theme, cx))
			.into_any_element()
	}

	fn gap_zone(
		&self,
		seed: u64,
		drop: Drop,
		inset: Rems,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let brand = theme.brand;
		div()
			.id(("rail-before", seed))
			.absolute()
			.top(-theme.space_8)
			.left(inset)
			.w(theme.server_icon)
			.h(theme.space_8)
			.rounded_full()
			.drag_over::<DraggedGuild>(move |style, _, _, _| style.bg(brand))
			.on_drop(
				cx.listener(move |this, dragged: &DraggedGuild, _, cx| this.drop_guild(dragged.id, drop, cx)),
			)
	}

	fn folder(&mut self, folder: &FolderItem, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = folder.id;
		let open = self.open_folders.contains(&id);
		let tint = with_alpha(folder.color, 0.4);
		if !open {
			let letters = folder
				.guilds
				.iter()
				.take(4)
				.map(|guild| MiniGuild {
					icon: guild.icon.clone(),
					letter: acronym(&guild.name).chars().take(1).collect::<String>().into(),
				})
				.collect();
			let unread = folder.guilds.iter().any(|guild| guild.unread);
			let mentions = folder.guilds.iter().map(|guild| guild.mentions).sum();
			let style = TileStyle {
				rest_background: tint,
				rest_foreground: theme.text_normal,
				active_background: tint,
				active_foreground: theme.text_normal,
			};
			let tile = self
				.tile(
					TileSpec {
						target: Target::Folder(id),
						face: Face::Folder(letters),
						style,
						unread,
						selected: false,
						mentions,
						pill_inset: rem(0.),
					},
					theme,
					cx,
				)
				.on_click(cx.listener(move |this, _, _, cx| this.toggle_folder(id, cx)))
				.tooltip(tooltip(folder.name.clone()));
			let inset = (theme.server_rail_width - theme.server_icon) / 2.;
			return self.drop_slot(
				tile,
				Target::Folder(id),
				(Drop::OntoFolder(id), Drop::BeforeFolder(id)),
				inset,
				theme,
				cx,
			);
		}
		let header_style = TileStyle {
			rest_background: tint,
			rest_foreground: folder.color,
			active_background: tint,
			active_foreground: folder.color,
		};
		let header = self
			.tile(
				TileSpec {
					target: Target::Folder(id),
					face: Face::Icon(IconKind::Folder),
					style: header_style,
					unread: false,
					selected: false,
					mentions: 0,
					pill_inset: rem(0.),
				},
				theme,
				cx,
			)
			.on_click(cx.listener(move |this, _, _, cx| this.toggle_folder(id, cx)))
			.tooltip(tooltip(folder.name.clone()));
		let header = self.drop_slot(
			header,
			Target::Folder(id),
			(Drop::OntoFolder(id), Drop::BeforeFolder(id)),
			rem(0.),
			theme,
			cx,
		);
		let guilds: Vec<AnyElement> = folder
			.guilds
			.iter()
			.map(|guild| self.guild_tile(guild, true, theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.items_center()
			.w_full()
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.w(theme.server_icon)
					.rounded(theme.radius_lg)
					.bg(tint)
					.child(header)
					.children(guilds),
			)
			.into_any_element()
	}
}

impl Render for ServerRail {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let home_style = TileStyle {
			rest_background: theme.background_primary,
			rest_foreground: theme.text_normal,
			active_background: theme.brand,
			active_foreground: theme.text_on_brand,
		};
		let add_style = || TileStyle {
			rest_background: theme.background_primary,
			rest_foreground: theme.status_online,
			active_background: theme.status_online,
			active_foreground: theme.text_on_brand,
		};
		let home = self
			.tile(
				TileSpec {
					target: Target::Home,
					face: Face::Icon(IconKind::Home),
					style: home_style,
					unread: false,
					selected: self.selected.is_none(),
					mentions: self.home_mentions,
					pill_inset: rem(0.),
				},
				&theme,
				cx,
			)
			.on_click(cx.listener(|_, _, _, cx| cx.emit(RailEvent::SelectHome)))
			.tooltip(tooltip(cx.t("rail-home")));
		let entries = std::mem::take(&mut self.entries);
		let rows: Vec<AnyElement> = entries
			.iter()
			.map(|entry| match entry {
				RailEntry::Guild(guild) => self.guild_tile(guild, false, &theme, cx),
				RailEntry::Folder(folder) => self.folder(folder, &theme, cx),
			})
			.collect();
		self.entries = entries;
		let add = self
			.tile(
				TileSpec {
					target: Target::AddServer,
					face: Face::Icon(IconKind::Plus),
					style: add_style(),
					unread: false,
					selected: false,
					mentions: 0,
					pill_inset: rem(0.),
				},
				&theme,
				cx,
			)
			.on_click(|_, window, cx| add_server::open(window, cx))
			.tooltip(tooltip(cx.t("rail-add-server")));
		let end_inset = (theme.server_rail_width - theme.server_icon) / 2.;
		let add = div()
			.relative()
			.flex_none()
			.w_full()
			.child(add)
			.child(self.gap_zone(Target::AddServer.seed(), Drop::End, end_inset, &theme, cx));
		let explore = self
			.tile(
				TileSpec {
					target: Target::Explore,
					face: Face::Icon(IconKind::Compass),
					style: add_style(),
					unread: false,
					selected: false,
					mentions: 0,
					pill_inset: rem(0.),
				},
				&theme,
				cx,
			)
			.on_click(|_, _, _| {
				if let Err(error) = platform::open::open_url(SERVER_DIRECTORY) {
					tracing::warn!(%error, "could not open the server directory");
				}
			})
			.tooltip(tooltip(cx.t("rail-explore")));
		let page = plugins::open_page(cx);
		let apps = plugins::app_nav(page.as_ref(), cx);
		div()
			.id("server-rail")
			.flex()
			.flex_col()
			.flex_none()
			.items_center()
			.gap(theme.space_8)
			.w(theme.server_rail_width)
			.h_full()
			.py(theme.space_12)
			.bg(theme.background_tertiary)
			.overflow_y_scroll()
			.child(home)
			.child(
				Divider::horizontal()
					.thickness(px(2.))
					.length(theme.space_32)
					.color(theme.background_primary),
			)
			.children(rows)
			.child(add)
			.child(explore)
			.when(!apps.is_empty(), |rail| {
				rail.child(
					Divider::horizontal()
						.thickness(px(2.))
						.length(theme.space_32)
						.color(theme.background_primary),
				)
				.children(apps)
			})
	}
}

fn guild_item(summary: &state::GuildSummary) -> GuildItem {
	GuildItem {
		id: summary.id,
		name: summary.name.clone().into(),
		icon: summary
			.icon
			.as_ref()
			.map(|hash| media::cdn::guild_icon_url(summary.id.get(), hash, ICON_PIXELS)),
		unread: summary.unread && !summary.muted,
		mentions: summary.mentions,
	}
}

fn rail_entries(state: &State) -> Vec<RailEntry> {
	let summaries = state.guild_summaries();
	let by_id: HashMap<Snowflake, &state::GuildSummary> =
		summaries.iter().map(|summary| (summary.id, summary)).collect();
	state
		.guild_layout()
		.into_iter()
		.filter_map(|entry| match entry {
			state::RailEntry::Guild(id) => by_id
				.get(&id)
				.map(|summary| RailEntry::Guild(guild_item(summary))),
			state::RailEntry::Folder {
				id,
				name,
				color,
				guilds,
			} => {
				let guilds: Vec<GuildItem> = guilds
					.iter()
					.filter_map(|id| by_id.get(id))
					.map(|summary| guild_item(summary))
					.collect();
				if guilds.is_empty() {
					return None;
				}
				let name = name.unwrap_or_else(|| {
					guilds
						.iter()
						.take(FOLDER_NAME_GUILDS)
						.map(|guild| guild.name.to_string())
						.collect::<Vec<_>>()
						.join(", ")
				});
				Some(RailEntry::Folder(FolderItem {
					id,
					name: name.into(),
					color: role_color(color.unwrap_or(DEFAULT_FOLDER_COLOR)),
					guilds,
				}))
			}
		})
		.collect()
}
