use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
	Window, div, prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Permissions, Role, Snowflake};
use session::Command;

use super::kit;
use super::rights::GROUPS;
use crate::components::{IconButton, PickHandler, ToggleHandler, field, hint, segmented, switch_row};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::model::role_color;
use crate::shell::community::access::GuildAccess;
use crate::theme::Theme;

const PALETTE: [u32; 20] = [
	0x1abc9c, 0x2ecc71, 0x3498db, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 0x95a5a6, 0x607d8b,
	0x11806a, 0x1f8b4c, 0x206694, 0x71368a, 0xad1457, 0xc27c0e, 0xa84300, 0x992d22, 0x979c9f, 0x546e7a,
];
const NEW_ROLE_NAME: &str = "new role";
const NAME_LIMIT: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
	Display,
	Permissions,
}

struct Draft {
	id: Snowflake,
	original: Role,
	color: u32,
	hoist: bool,
	mentionable: bool,
	permissions: Permissions,
}

pub struct RolesPage {
	data: Entity<Data>,
	guild: Snowflake,
	draft: Option<Draft>,
	tab: Tab,
	name: Entity<InputState>,
	hex: Entity<InputState>,
	search: Entity<InputState>,
	_subscriptions: Vec<Subscription>,
}

fn color_of(role: &Role) -> u32 {
	let value = if role.color != 0 {
		role.color
	} else {
		role.colors
			.as_ref()
			.and_then(|colors| colors.primary_color)
			.unwrap_or(0)
	};
	u32::try_from(value).unwrap_or(0)
}

fn parse_hex(text: &str) -> Option<u32> {
	let digits = text.trim().trim_start_matches('#');
	(digits.len() == 6)
		.then(|| u32::from_str_radix(digits, 16).ok())
		.flatten()
}

impl RolesPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let name = cx.new(|cx| InputState::new(window, cx));
		let hex = cx.new(|cx| InputState::new(window, cx).placeholder("#99aab5"));
		let search_placeholder = cx.t("roles-search-permissions");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(search_placeholder));
		let subscriptions = vec![
			cx.subscribe_in(&name, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
			cx.subscribe_in(&hex, window, |this, input, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change)
					&& let Some(color) = parse_hex(&input.read(cx).value())
					&& let Some(draft) = &mut this.draft
				{
					draft.color = color;
					cx.notify();
				}
			}),
			cx.subscribe_in(&search, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
		];
		Self {
			data,
			guild,
			draft: None,
			tab: Tab::Display,
			name,
			hex,
			search,
			_subscriptions: subscriptions,
		}
	}

	fn roles_sorted(&self, cx: &App) -> Vec<Role> {
		let state = self.data.read(cx).state();
		let mut roles: Vec<Role> = state
			.guild(self.guild)
			.map(|guild| guild.roles.values().cloned().collect())
			.unwrap_or_default();
		roles.sort_by_key(|role| std::cmp::Reverse((role.id != self.guild, role.position, role.id)));
		roles
	}

	fn member_count(&self, role: &Role, cx: &App) -> usize {
		let state = self.data.read(cx).state();
		state.guild(self.guild).map_or(0, |guild| {
			if role.id == guild.id {
				guild.members.len()
			} else {
				guild
					.members
					.values()
					.filter(|member| member.roles.contains(&role.id))
					.count()
			}
		})
	}

	fn begin(&mut self, role: Role, window: &mut Window, cx: &mut Context<Self>) {
		let name = role.name.clone();
		let color = color_of(&role);
		self.name
			.update(cx, |state, cx| state.set_value(name, window, cx));
		self.hex.update(cx, |state, cx| {
			state.set_value(
				if color == 0 {
					String::new()
				} else {
					format!("#{color:06x}")
				},
				window,
				cx,
			)
		});
		self.search
			.update(cx, |state, cx| state.set_value("", window, cx));
		self.tab = Tab::Display;
		self.draft = Some(Draft {
			id: role.id,
			color,
			hoist: role.hoist,
			mentionable: role.mentionable,
			permissions: role.permissions,
			original: role,
		});
		cx.notify();
	}

	fn dirty(&self, cx: &App) -> bool {
		let Some(draft) = &self.draft else {
			return false;
		};
		let name = self.name.read(cx).value();
		let name = name.trim();
		let name_changed = draft.original.id != self.guild && name != draft.original.name;
		name_changed
			|| draft.color != color_of(&draft.original)
			|| draft.hoist != draft.original.hoist
			|| draft.mentionable != draft.original.mentionable
			|| draft.permissions != draft.original.permissions
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
		let Some(draft) = &mut self.draft else {
			return;
		};
		let original = draft.original.clone();
		self.data.read(cx).send(Command::EditRole {
			guild_id: self.guild,
			role_id: draft.id,
			name: (name != original.name && !name.is_empty() && original.id != self.guild)
				.then_some(name.clone()),
			color: (draft.color != color_of(&original)).then_some(draft.color),
			permissions: (draft.permissions != original.permissions).then_some(draft.permissions),
			hoist: (draft.hoist != original.hoist).then_some(draft.hoist),
			mentionable: (draft.mentionable != original.mentionable).then_some(draft.mentionable),
		});
		draft.original.permissions = draft.permissions;
		draft.original.hoist = draft.hoist;
		draft.original.mentionable = draft.mentionable;
		draft.original.color = i64::from(draft.color);
		draft.original.colors = None;
		if !name.is_empty() && original.id != self.guild {
			draft.original.name = name;
		}
		cx.notify();
	}

	fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(role) = self.draft.as_ref().map(|draft| draft.original.clone()) {
			self.begin(role, window, cx);
		}
	}

	fn create(&mut self, cx: &mut Context<Self>) {
		let permissions = {
			let state = self.data.read(cx).state();
			state
				.guild(self.guild)
				.and_then(|guild| guild.roles.get(&self.guild))
				.map_or(Permissions::empty(), |role| role.permissions)
		};
		self.data.read(cx).send(Command::CreateRole {
			guild_id: self.guild,
			name: NEW_ROLE_NAME.to_owned(),
			color: 0,
			permissions,
			hoist: false,
			mentionable: false,
		});
	}

	fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(draft) = &self.draft else {
			return;
		};
		let data = self.data.clone();
		let guild = self.guild;
		let role_id = draft.id;
		let weak = cx.weak_entity();
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			let weak = weak.clone();
			alert
				.title(cx.t("roles-delete-title"))
				.description(cx.t("roles-delete-body"))
				.confirm()
				.ok_text(cx.t("roles-delete"))
				.ok_variant(ButtonVariant::Danger)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::DeleteRole {
						guild_id: guild,
						role_id,
					});
					weak.update(cx, |this, cx| {
						this.draft = None;
						cx.notify();
					})
					.ok();
					true
				})
		});
	}

	fn move_role(&mut self, id: Snowflake, up: bool, cx: &mut Context<Self>) {
		let roles = self.roles_sorted(cx);
		let Some(index) = roles.iter().position(|role| role.id == id) else {
			return;
		};
		let neighbor = if up { index.checked_sub(1) } else { Some(index + 1) };
		let Some(other) = neighbor.and_then(|neighbor| roles.get(neighbor)) else {
			return;
		};
		if other.id == self.guild {
			return;
		}
		self.data.read(cx).send(Command::ReorderRoles {
			guild_id: self.guild,
			positions: vec![(id, other.position), (other.id, roles[index].position)],
		});
	}

	fn role_row(
		&self,
		role: &Role,
		access: &GuildAccess,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let id = role.id;
		let count = self.member_count(role, cx);
		let color = match color_of(role) {
			0 => theme.interactive_muted,
			value => role_color(value),
		};
		let editable = role.id == self.guild || access.owner || role.position < access.top_role;
		let managed = role.id != self.guild && editable;
		let role_clone = role.clone();
		div()
			.id(("role-row", id.get()))
			.flex()
			.items_center()
			.gap(theme.space_12)
			.px(theme.space_12)
			.h(theme.list_row_height)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_hover))
			.on_click(cx.listener(move |this, _, window, cx| this.begin(role_clone.clone(), window, cx)))
			.child(Icon::new(IconKind::Shield).size(theme.glyph_md).color(color))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.truncate()
					.text_size(theme.font_md)
					.font_weight(FontWeight::MEDIUM)
					.text_color(theme.header_primary)
					.child(role.name.clone()),
			)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_4)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(
						Icon::new(IconKind::Members)
							.size(theme.glyph_sm)
							.color(theme.text_muted),
					)
					.child(count.to_string()),
			)
			.when(managed, |row| {
				row.child(
					IconButton::new(("role-up", id.get()), IconKind::ArrowUp)
						.tooltip(cx.t("roles-move-up"))
						.on_click(cx.listener(move |this, _, _, cx| {
							cx.stop_propagation();
							this.move_role(id, true, cx);
						})),
				)
				.child(
					IconButton::new(("role-down", id.get()), IconKind::ArrowDown)
						.tooltip(cx.t("roles-move-down"))
						.on_click(cx.listener(move |this, _, _, cx| {
							cx.stop_propagation();
							this.move_role(id, false, cx);
						})),
				)
			})
			.into_any_element()
	}

	fn list(&self, access: &GuildAccess, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let roles = self.roles_sorted(cx);
		let rows: Vec<AnyElement> = roles
			.iter()
			.map(|role| self.role_row(role, access, theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-roles"), theme))
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.justify_between()
					.pb(theme.space_16)
					.child(hint(
						cx.t_args("roles-count", &[("count", ArgValue::from(roles.len()))]),
						theme,
					))
					.child(
						Button::new("roles-create")
							.primary()
							.icon(gpui_component::Icon::default().path(IconKind::Plus.path()))
							.label(cx.t("roles-create"))
							.on_click(cx.listener(|this, _, _, cx| this.create(cx))),
					),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_2)
					.pb(theme.space_32)
					.children(rows)
					.overflow_y_scrollbar()
					.id("roles-list")
					.flex_1()
					.min_h_0(),
			)
			.into_any_element()
	}

	fn swatch(&self, color: u32, active: bool, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		div()
			.id(("role-swatch", color as usize))
			.size(theme.settings_swatch)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.bg(role_color(color))
			.border_2()
			.border_color(if active {
				theme.interactive_active
			} else {
				theme.background_primary
			})
			.on_click(cx.listener(move |this, _, window, cx| {
				if let Some(draft) = &mut this.draft {
					draft.color = color;
				}
				this.hex.update(cx, |state, cx| {
					state.set_value(format!("#{color:06x}"), window, cx)
				});
				cx.notify();
			}))
			.into_any_element()
	}

	fn display_tab(&self, draft: &Draft, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let swatches: Vec<AnyElement> = PALETTE
			.iter()
			.map(|color| self.swatch(*color, draft.color == *color, theme, cx))
			.collect();
		let weak = cx.weak_entity();
		let hoist: ToggleHandler = {
			let weak = weak.clone();
			Rc::new(move |value, _, cx| {
				weak.update(cx, |this, cx| {
					if let Some(draft) = &mut this.draft {
						draft.hoist = value;
					}
					cx.notify();
				})
				.ok();
			})
		};
		let mentionable: ToggleHandler = Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| {
				if let Some(draft) = &mut this.draft {
					draft.mentionable = value;
				}
				cx.notify();
			})
			.ok();
		});
		let everyone = draft.id == self.guild;
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.when(!everyone, |tab| {
				tab.child(field(cx.t("roles-name"), theme, Input::new(&self.name)))
			})
			.child(field(
				cx.t("roles-color"),
				theme,
				div()
					.flex()
					.flex_col()
					.gap(theme.space_12)
					.child(div().flex().flex_wrap().gap(theme.space_8).children(swatches))
					.child(div().w(theme.settings_nav_width).child(Input::new(&self.hex))),
			))
			.child(switch_row(
				"roles-hoist",
				cx.t("roles-hoist"),
				Some(cx.t("roles-hoist-hint")),
				draft.hoist,
				theme,
				hoist,
			))
			.child(switch_row(
				"roles-mentionable",
				cx.t("roles-mentionable"),
				Some(cx.t("roles-mentionable-hint")),
				draft.mentionable,
				theme,
				mentionable,
			))
			.into_any_element()
	}

	fn permissions_tab(&self, draft: &Draft, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let query = self.search.read(cx).value().trim().to_lowercase();
		let mut sections: Vec<AnyElement> = Vec::new();
		for group in GROUPS {
			let rows: Vec<AnyElement> = group
				.rights
				.iter()
				.filter(|right| query.is_empty() || cx.t(right.name).to_lowercase().contains(&query))
				.map(|right| {
					let flag = right.flag;
					let weak = cx.weak_entity();
					let toggle: ToggleHandler = Rc::new(move |value, _, cx| {
						weak.update(cx, |this, cx| {
							if let Some(draft) = &mut this.draft {
								draft.permissions.set(flag, value);
							}
							cx.notify();
						})
						.ok();
					});
					div()
						.py(theme.space_8)
						.border_b_1()
						.border_color(theme.divider)
						.child(switch_row(
							("role-right", flag.bits() as usize),
							cx.t(right.name),
							Some(cx.t(right.hint)),
							draft.permissions.contains(flag),
							theme,
							toggle,
						))
						.into_any_element()
				})
				.collect();
			if rows.is_empty() {
				continue;
			}
			sections.push(
				div()
					.flex()
					.flex_col()
					.child(kit::count_caption(cx.t(group.key), theme))
					.children(rows)
					.into_any_element(),
			);
		}
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(Input::new(&self.search))
			.children(sections)
			.into_any_element()
	}

	fn editor(&self, draft: &Draft, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let dirty = self.dirty(cx);
		let weak_reset = cx.weak_entity();
		let weak_save = cx.weak_entity();
		let labels = [cx.t("roles-tab-display"), cx.t("roles-tab-permissions")];
		let weak_tab = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak_tab
				.update(cx, |this, cx| {
					this.tab = if index == 0 {
						Tab::Display
					} else {
						Tab::Permissions
					};
					cx.notify();
				})
				.ok();
		});
		let deletable = draft.id != self.guild && !draft.original.managed;
		let body = match self.tab {
			Tab::Display => self.display_tab(draft, theme, cx),
			Tab::Permissions => self.permissions_tab(draft, theme, cx),
		};
		let title: SharedString = if draft.id == self.guild {
			cx.t("roles-everyone")
		} else {
			draft.original.name.clone().into()
		};
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_12)
					.pb(theme.space_20)
					.child(
						IconButton::new("roles-back", IconKind::ChevronLeft)
							.tooltip(cx.t("roles-back"))
							.on_click(cx.listener(|this, _, _, cx| {
								this.draft = None;
								cx.notify();
							})),
					)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.text_size(theme.font_xl)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_primary)
							.child(title),
					)
					.when(deletable, |row| {
						row.child(
							Button::new("roles-delete")
								.danger()
								.outline()
								.label(cx.t("roles-delete"))
								.on_click(cx.listener(|this, _, window, cx| this.confirm_delete(window, cx))),
						)
					}),
			)
			.child(div().flex_none().pb(theme.space_16).child(segmented(
				"roles-tabs",
				&labels,
				usize::from(self.tab == Tab::Permissions),
				theme,
				pick,
			)))
			.child(
				div()
					.flex()
					.flex_col()
					.pb(theme.space_32)
					.child(body)
					.overflow_y_scrollbar()
					.id("roles-editor")
					.flex_1()
					.min_h_0(),
			)
			.when(dirty, |editor| {
				editor.child(kit::save_bar(
					theme,
					cx,
					move |window, cx| {
						weak_reset.update(cx, |this, cx| this.reset(window, cx)).ok();
					},
					move |_, cx| {
						weak_save.update(cx, |this, cx| this.save(cx)).ok();
					},
				))
			})
			.into_any_element()
	}
}

impl Render for RolesPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let access = GuildAccess::of(&self.data.read(cx).state(), self.guild);
		match &self.draft {
			Some(draft) => {
				let draft_view = Draft {
					id: draft.id,
					original: draft.original.clone(),
					color: draft.color,
					hoist: draft.hoist,
					mentionable: draft.mentionable,
					permissions: draft.permissions,
				};
				self.editor(&draft_view, &theme, cx)
			}
			None => self.list(&access, &theme, cx),
		}
	}
}
