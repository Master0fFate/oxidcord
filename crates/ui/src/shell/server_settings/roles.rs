use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ObjectFit, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
	StyledImage as _, Subscription, Window, div, img, prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Permissions, Role, Snowflake};
use session::{Command, RoleColorsEdit, RoleIconEdit};
use state::{ENHANCED_ROLE_COLORS, ROLE_ICONS, RoleIcon};

use super::kit;
use super::rights::GROUPS;
use crate::components::{
	COLOR_PALETTE, IconButton, PickHandler, ToggleHandler, color_swatch, field, hint, parse_hex_color,
	role_icon, segmented, switch_row, unicode_emoji,
};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::model::role_color;
use crate::shell::community::access::GuildAccess;
use crate::shell::community::files::{Fit, IMAGE_TYPES, Picked, pick_image};
use crate::shell::emoji_picker::{EmojiChoice, EmojiPicker, EmojiPickerEvent};
use crate::theme::Theme;

const NEW_ROLE_NAME: &str = "new role";
const NAME_LIMIT: usize = 100;
const EMPTY_COUNT: &str = "-";
const ICON_SIDE: u32 = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
	Display,
	Permissions,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
	Solid,
	Gradient,
	Holographic,
}

impl Style {
	const ALL: [Self; 3] = [Self::Solid, Self::Gradient, Self::Holographic];

	fn of(colors: RoleColorsEdit) -> Self {
		if colors.tertiary.is_some() {
			Self::Holographic
		} else if colors.secondary.is_some() {
			Self::Gradient
		} else {
			Self::Solid
		}
	}

	fn label(self) -> &'static str {
		match self {
			Self::Solid => "roles-style-solid",
			Self::Gradient => "roles-style-gradient",
			Self::Holographic => "roles-style-holographic",
		}
	}
}

enum IconDraft {
	Keep,
	Remove,
	Image(Picked),
	Emoji(String),
}

struct Draft {
	id: Snowflake,
	original: Role,
	color: u32,
	secondary: u32,
	style: Style,
	enhanced: bool,
	icon: IconDraft,
	hoist: bool,
	mentionable: bool,
	permissions: Permissions,
}

impl Draft {
	fn colors(&self) -> RoleColorsEdit {
		match self.style {
			Style::Solid => RoleColorsEdit::solid(self.color),
			Style::Gradient => RoleColorsEdit {
				primary: self.color,
				secondary: Some(self.secondary),
				tertiary: None,
			},
			Style::Holographic => RoleColorsEdit::HOLOGRAPHIC,
		}
	}

	fn colors_changed(&self) -> bool {
		let original = RoleColorsEdit::from(&self.original);
		if self.enhanced {
			self.colors() != original
		} else {
			self.color != original.primary
		}
	}

	fn icon_edit(&self) -> Option<RoleIconEdit> {
		match &self.icon {
			IconDraft::Keep => None,
			IconDraft::Remove => Some(RoleIconEdit::Remove),
			IconDraft::Image(picked) => Some(RoleIconEdit::Image(picked.bytes.clone())),
			IconDraft::Emoji(glyph) => Some(RoleIconEdit::Emoji(glyph.clone())),
		}
	}
}

pub struct RolesPage {
	data: Entity<Data>,
	guild: Snowflake,
	draft: Option<Draft>,
	tab: Tab,
	name: Entity<InputState>,
	hex: Entity<InputState>,
	search: Entity<InputState>,
	picker: Option<Entity<EmojiPicker>>,
	picker_open: bool,
	_subscriptions: Vec<Subscription>,
}

impl RolesPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		data.read(cx).send(Command::LoadRoleCounts { guild_id: guild });
		let name = cx.new(|cx| InputState::new(window, cx));
		let hex = cx.new(|cx| InputState::new(window, cx).placeholder("#99aab5"));
		let search_placeholder = cx.t("roles-search-permissions");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(search_placeholder));
		let subscriptions = vec![
			cx.subscribe(&data, |this, _, event: &DataEvent, cx| {
				if matches!(event, DataEvent::Changed(_)) {
					this.sync_original(cx);
					cx.notify();
				}
			}),
			cx.subscribe_in(&name, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
			cx.subscribe_in(&hex, window, |this, input, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change)
					&& let Some(color) = parse_hex_color(&input.read(cx).value())
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
			picker: None,
			picker_open: false,
			_subscriptions: subscriptions,
		}
	}

	fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.picker.is_none() {
			let picker = cx.new(|cx| EmojiPicker::new(window, cx));
			picker.update(cx, |picker, cx| picker.open(Vec::new(), window, cx));
			self._subscriptions
				.push(cx.subscribe(&picker, |this, _, event: &EmojiPickerEvent, cx| {
					let EmojiPickerEvent::Pick(choice) = event;
					if let EmojiChoice::Unicode(glyph) = choice
						&& let Some(draft) = &mut this.draft
					{
						draft.icon = IconDraft::Emoji(glyph.clone());
						this.picker_open = false;
						cx.notify();
					}
				}));
			self.picker = Some(picker);
		}
		self.picker_open = !self.picker_open;
		cx.notify();
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

	fn member_counts(&self, roles: &[Role], cx: &App) -> Vec<Option<u64>> {
		let state = self.data.read(cx).state();
		let counts = state.role_member_counts(self.guild);
		let total = state
			.guild(self.guild)
			.and_then(|guild| guild.member_count)
			.and_then(|total| u64::try_from(total).ok());
		roles
			.iter()
			.map(|role| {
				let everyone = if role.id == self.guild { total } else { None };
				everyone.or_else(|| counts.and_then(|counts| counts.get(&role.id).copied()))
			})
			.collect()
	}

	fn begin(&mut self, role: Role, window: &mut Window, cx: &mut Context<Self>) {
		let name = role.name.clone();
		let colors = RoleColorsEdit::from(&role);
		let color = colors.primary;
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
		self.picker_open = false;
		let enhanced = self
			.data
			.read(cx)
			.state()
			.guild(self.guild)
			.is_some_and(|guild| guild.has_feature(ENHANCED_ROLE_COLORS));
		self.draft = Some(Draft {
			id: role.id,
			color,
			secondary: colors.secondary.unwrap_or(COLOR_PALETTE[0]),
			style: if enhanced { Style::of(colors) } else { Style::Solid },
			enhanced,
			icon: IconDraft::Keep,
			hoist: role.hoist,
			mentionable: role.mentionable,
			permissions: role.permissions,
			original: role,
		});
		cx.notify();
	}

	fn sync_original(&mut self, cx: &App) {
		let Some(draft) = &mut self.draft else {
			return;
		};
		let state = self.data.read(cx).state();
		if let Some(role) = state
			.guild(self.guild)
			.and_then(|guild| guild.roles.get(&draft.id))
			.filter(|role| **role != draft.original)
		{
			draft.original = role.clone();
		}
	}

	fn dirty(&self, cx: &App) -> bool {
		let Some(draft) = &self.draft else {
			return false;
		};
		let name = self.name.read(cx).value();
		let name = name.trim();
		let name_changed = draft.original.id != self.guild && name != draft.original.name;
		name_changed
			|| draft.colors_changed()
			|| !matches!(draft.icon, IconDraft::Keep)
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
		let original = &draft.original;
		let colors = draft.colors();
		self.data.read(cx).send(Command::EditRole {
			guild_id: self.guild,
			role_id: draft.id,
			name: (name != original.name && !name.is_empty() && original.id != self.guild).then_some(name),
			colors: draft.colors_changed().then_some(colors),
			icon: draft.icon_edit(),
			permissions: (draft.permissions != original.permissions).then_some(draft.permissions),
			hoist: (draft.hoist != original.hoist).then_some(draft.hoist),
			mentionable: (draft.mentionable != original.mentionable).then_some(draft.mentionable),
		});
		draft.icon = IconDraft::Keep;
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
			colors: RoleColorsEdit::solid(0),
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
		count: Option<u64>,
		access: &GuildAccess,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let id = role.id;
		let color = match role.primary_color() {
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
					.child(count.map_or_else(|| EMPTY_COUNT.to_owned(), |count| count.to_string())),
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
		let counts = self.member_counts(&roles, cx);
		let rows: Vec<AnyElement> = roles
			.iter()
			.zip(counts)
			.map(|(role, count)| self.role_row(role, count, access, theme, cx))
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
		color_swatch(
			color,
			active,
			theme,
			cx.listener(move |this, _, window, cx| {
				if let Some(draft) = &mut this.draft {
					draft.color = color;
				}
				this.hex.update(cx, |state, cx| {
					state.set_value(format!("#{color:06x}"), window, cx)
				});
				cx.notify();
			}),
		)
	}

	fn second_swatch(&self, color: u32, active: bool, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		color_swatch(
			color,
			active,
			theme,
			cx.listener(move |this, _, _, cx| {
				if let Some(draft) = &mut this.draft {
					draft.secondary = color;
				}
				cx.notify();
			}),
		)
	}

	fn style_field(&self, draft: &Draft, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let labels: Vec<SharedString> = Style::ALL.iter().map(|style| cx.t(style.label())).collect();
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				if let Some(draft) = &mut this.draft {
					draft.style = Style::ALL[index];
				}
				cx.notify();
			})
			.ok();
		});
		let selected = Style::ALL
			.iter()
			.position(|style| *style == draft.style)
			.unwrap_or(0);
		field(
			cx.t("roles-style"),
			theme,
			segmented("roles-style", &labels, selected, theme, pick),
		)
	}

	fn color_fields(&self, draft: &Draft, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
		if draft.style == Style::Holographic {
			return Vec::new();
		}
		let swatches: Vec<AnyElement> = COLOR_PALETTE
			.iter()
			.map(|color| self.swatch(*color, draft.color == *color, theme, cx))
			.collect();
		let mut fields = vec![field(
			cx.t("roles-color"),
			theme,
			div()
				.flex()
				.flex_col()
				.gap(theme.space_12)
				.child(div().flex().flex_wrap().gap(theme.space_8).children(swatches))
				.child(div().w(theme.settings_nav_width).child(Input::new(&self.hex))),
		)];
		if draft.style == Style::Gradient {
			let second: Vec<AnyElement> = COLOR_PALETTE
				.iter()
				.map(|color| self.second_swatch(*color, draft.secondary == *color, theme, cx))
				.collect();
			fields.push(field(
				cx.t("roles-color-second"),
				theme,
				div().flex().flex_wrap().gap(theme.space_8).children(second),
			));
		}
		fields
	}

	fn icon_field(&self, draft: &Draft, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let current = RoleIcon::of(&draft.original);
		let shown = match &draft.icon {
			IconDraft::Keep => current.as_ref().map(|icon| role_icon(icon, theme.icon_lg, cx)),
			IconDraft::Remove => None,
			IconDraft::Image(picked) => Some(
				div()
					.size(theme.icon_lg)
					.child(
						img(picked.preview.clone())
							.size_full()
							.object_fit(ObjectFit::Contain),
					)
					.into_any_element(),
			),
			IconDraft::Emoji(glyph) => Some(unicode_emoji(glyph, theme.icon_lg, cx)),
		};
		let removable = shown.is_some();
		let keeps_current = current.is_some();
		field(
			cx.t("roles-icon"),
			theme,
			div()
				.flex()
				.flex_col()
				.gap(theme.space_12)
				.child(
					div()
						.flex()
						.items_center()
						.gap(theme.space_12)
						.child(
							div()
								.flex()
								.flex_none()
								.items_center()
								.justify_center()
								.size(theme.field_height)
								.rounded(theme.radius_sm)
								.bg(theme.input_background)
								.children(shown),
						)
						.child(
							Button::new("role-icon-upload")
								.outline()
								.label(cx.t("roles-icon-upload"))
								.on_click(cx.listener(|_, _, _, cx| {
									pick_image(
										cx,
										Fit::Square(ICON_SIDE),
										IMAGE_TYPES,
										|this: &mut Self, picked, cx| {
											if let Some(draft) = &mut this.draft {
												draft.icon = IconDraft::Image(picked);
												cx.notify();
											}
										},
									)
								})),
						)
						.child(
							Button::new("role-icon-emoji")
								.outline()
								.label(cx.t("roles-icon-emoji"))
								.on_click(cx.listener(|this, _, window, cx| this.toggle_picker(window, cx))),
						)
						.when(removable, |row| {
							row.child(
								Button::new("role-icon-remove")
									.ghost()
									.label(cx.t("roles-icon-remove"))
									.on_click(cx.listener(move |this, _, _, cx| {
										if let Some(draft) = &mut this.draft {
											draft.icon = if keeps_current {
												IconDraft::Remove
											} else {
												IconDraft::Keep
											};
										}
										cx.notify();
									})),
							)
						}),
				)
				.when_some(
					self.picker.clone().filter(|_| self.picker_open),
					|column, picker| column.child(picker),
				),
		)
	}

	fn display_tab(&self, draft: &Draft, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
		let (enhanced_colors, icons) =
			self.data
				.read(cx)
				.state()
				.guild(self.guild)
				.map_or((false, false), |guild| {
					(
						guild.has_feature(ENHANCED_ROLE_COLORS),
						guild.has_feature(ROLE_ICONS),
					)
				});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.when(!everyone, |tab| {
				tab.child(field(cx.t("roles-name"), theme, Input::new(&self.name)))
			})
			.when(enhanced_colors, |tab| {
				tab.child(self.style_field(draft, theme, cx))
			})
			.children(self.color_fields(draft, theme, cx))
			.when(icons && !everyone, |tab| {
				tab.child(self.icon_field(draft, theme, cx))
			})
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
			Some(draft) => self.editor(draft, &theme, cx),
			None => self.list(&access, &theme, cx),
		}
	}
}
