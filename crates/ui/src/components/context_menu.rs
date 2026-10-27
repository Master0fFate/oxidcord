use std::rc::Rc;

use gpui::{
	AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement as _, MouseButton,
	ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, anchored,
	deferred, div, prelude::FluentBuilder as _, px,
};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const MENU_WIDTH: f32 = 220.;
const ROW_HEIGHT: f32 = 28.;
const ICON_SIZE: f32 = 20.;
const TRAILING_ICON_SIZE: f32 = 18.;
const ITEM_INSET_X: f32 = 8.;
const ITEM_INSET_Y: f32 = 4.;
const PANEL_PADDING: f32 = 4.;
const FLYOUT_OVERLAP: f32 = 2.;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;
type Render = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

pub enum MenuEntry {
	Item {
		label: SharedString,
		icon: Option<IconKind>,
		trailing: Option<IconKind>,
		danger: bool,
		disabled: bool,
		checked: Option<bool>,
		on_select: Handler,
	},
	Submenu {
		label: SharedString,
		icon: Option<IconKind>,
		entries: Rc<Vec<MenuEntry>>,
	},
	Lazy(Rc<dyn Fn(&App) -> ContextMenuBuilder>),
	Element {
		render: Render,
		on_select: Option<Handler>,
	},
	Separator,
}

#[derive(Default)]
pub struct ContextMenuBuilder {
	entries: Vec<MenuEntry>,
}

impl ContextMenuBuilder {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn item(
		self,
		label: impl Into<SharedString>,
		icon: Option<IconKind>,
		on_select: impl Fn(&mut Window, &mut App) + 'static,
	) -> Self {
		self.entry(label, icon, false, on_select)
	}

	pub fn disabled_item(mut self, label: impl Into<SharedString>, icon: Option<IconKind>) -> Self {
		self.entries.push(MenuEntry::Item {
			label: label.into(),
			icon,
			trailing: None,
			danger: false,
			disabled: true,
			checked: None,
			on_select: Rc::new(|_, _| {}),
		});
		self
	}

	pub fn danger_item(
		self,
		label: impl Into<SharedString>,
		icon: Option<IconKind>,
		on_select: impl Fn(&mut Window, &mut App) + 'static,
	) -> Self {
		self.entry(label, icon, true, on_select)
	}

	pub fn external_item(
		mut self,
		label: impl Into<SharedString>,
		icon: Option<IconKind>,
		on_select: impl Fn(&mut Window, &mut App) + 'static,
	) -> Self {
		self.entries.push(MenuEntry::Item {
			label: label.into(),
			icon,
			trailing: Some(IconKind::ExternalLink),
			danger: false,
			disabled: false,
			checked: None,
			on_select: Rc::new(on_select),
		});
		self
	}

	pub fn separator(mut self) -> Self {
		self.entries.push(MenuEntry::Separator);
		self
	}

	pub fn element(mut self, render: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
		self.entries.push(MenuEntry::Element {
			render: Rc::new(render),
			on_select: None,
		});
		self
	}

	pub fn element_item(
		mut self,
		render: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
		on_select: impl Fn(&mut Window, &mut App) + 'static,
	) -> Self {
		self.entries.push(MenuEntry::Element {
			render: Rc::new(render),
			on_select: Some(Rc::new(on_select)),
		});
		self
	}

	pub fn checked_item(
		mut self,
		label: impl Into<SharedString>,
		checked: bool,
		on_select: impl Fn(&mut Window, &mut App) + 'static,
	) -> Self {
		self.entries.push(MenuEntry::Item {
			label: label.into(),
			icon: None,
			trailing: None,
			danger: false,
			disabled: false,
			checked: Some(checked),
			on_select: Rc::new(on_select),
		});
		self
	}

	pub fn submenu(mut self, label: impl Into<SharedString>, icon: Option<IconKind>, menu: Self) -> Self {
		self.entries.push(MenuEntry::Submenu {
			label: label.into(),
			icon,
			entries: Rc::new(menu.entries),
		});
		self
	}

	pub fn lazy(mut self, build: impl Fn(&App) -> ContextMenuBuilder + 'static) -> Self {
		self.entries.push(MenuEntry::Lazy(Rc::new(build)));
		self
	}

	pub fn extend(mut self, other: Self) -> Self {
		self.entries.extend(other.entries);
		self
	}

	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	pub fn build(self) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
		let entries = Rc::new(self.entries);
		move |menu, window, cx| {
			populate(
				menu.min_w(rem(MENU_WIDTH).to_pixels(window.rem_size())),
				&entries,
				cx,
			)
		}
	}

	fn entry(
		mut self,
		label: impl Into<SharedString>,
		icon: Option<IconKind>,
		danger: bool,
		on_select: impl Fn(&mut Window, &mut App) + 'static,
	) -> Self {
		self.entries.push(MenuEntry::Item {
			label: label.into(),
			icon,
			trailing: None,
			danger,
			disabled: false,
			checked: None,
			on_select: Rc::new(on_select),
		});
		self
	}
}

fn populate(menu: PopupMenu, entries: &[MenuEntry], cx: &mut Context<PopupMenu>) -> PopupMenu {
	entries.iter().fold(menu, |menu, entry| match entry {
		MenuEntry::Separator => menu.separator(),
		MenuEntry::Element { render, on_select } => {
			let render = render.clone();
			let item = PopupMenuItem::element(move |window, cx| render(window, cx));
			menu.item(match on_select.clone() {
				Some(on_select) => item.on_click(move |_, window, cx| on_select(window, cx)),
				None => item,
			})
		}
		MenuEntry::Lazy(build) => {
			let built = build(cx);
			populate(menu, &built.entries, cx)
		}
		MenuEntry::Submenu { label, icon, entries } => {
			let row = Row {
				label: label.clone(),
				icon: *icon,
				trailing: None,
				danger: false,
				disabled: false,
				checked: false,
			};
			let entries = entries.clone();
			menu.item(
				PopupMenuItem::element(move |window, cx| submenu_row(&row, &entries, window, cx))
					.disabled(true),
			)
		}
		MenuEntry::Item {
			label,
			icon,
			trailing,
			danger,
			disabled,
			checked,
			on_select,
		} => {
			let on_select = on_select.clone();
			let row = Row {
				label: label.clone(),
				icon: *icon,
				trailing: *trailing,
				danger: *danger,
				disabled: *disabled,
				checked: checked.unwrap_or(false),
			};
			menu.item(
				PopupMenuItem::element(move |_, cx| row.render(cx))
					.disabled(*disabled)
					.on_click(move |_, window, cx| on_select(window, cx)),
			)
		}
	})
}

struct Row {
	label: SharedString,
	icon: Option<IconKind>,
	trailing: Option<IconKind>,
	danger: bool,
	disabled: bool,
	checked: bool,
}

impl Row {
	fn render(&self, cx: &App) -> AnyElement {
		let theme = cx.global::<Theme>();
		let color = if self.danger {
			theme.danger
		} else if self.disabled {
			theme.text_muted
		} else {
			theme.text_normal
		};
		div()
			.flex()
			.items_center()
			.gap(rem(10.))
			.w_full()
			.h(rem(ROW_HEIGHT))
			.text_size(theme.font_sm)
			.font_weight(FontWeight::MEDIUM)
			.text_color(color)
			.children(
				self.icon
					.map(|kind| Icon::new(kind).size(rem(ICON_SIZE)).color(color)),
			)
			.child(div().flex_1().min_w_0().truncate().child(self.label.clone()))
			.children(
				self.trailing
					.map(|kind| Icon::new(kind).size(rem(TRAILING_ICON_SIZE)).color(color)),
			)
			.when(self.checked, |row| {
				row.child(Icon::new(IconKind::Check).size(rem(18.)).color(theme.brand))
			})
			.into_any_element()
	}
}

#[derive(Clone, Copy, Default)]
struct Hover {
	row: bool,
	flyout: bool,
}

fn submenu_row(row: &Row, entries: &Rc<Vec<MenuEntry>>, window: &mut Window, cx: &mut App) -> AnyElement {
	let key = SharedString::from(format!("menu-submenu-{}", row.label));
	let hover = window.use_keyed_state(key.clone(), cx, |_, _| Hover::default());
	let open = {
		let state = hover.read(cx);
		state.row || state.flyout
	};
	let theme = cx.global::<Theme>();
	let highlight = theme.modifier_selected;
	let chevron = theme.text_muted;
	let radius = theme.radius_sm;
	let row_hover = hover.clone();
	let flyout = open.then(|| {
		let flyout_hover = hover.clone();
		div()
			.absolute()
			.top(rem(-PANEL_PADDING))
			.right(rem(-FLYOUT_OVERLAP))
			.child(
				deferred(
					anchored().snap_to_window_with_margin(px(8.)).child(
						div()
							.id(SharedString::from(format!("{key}-flyout")))
							.occlude()
							.on_hover(move |hovered, _, cx| {
								flyout_hover.update(cx, |state, cx| {
									state.flyout = *hovered;
									cx.notify();
								})
							})
							.child(panel(entries, window, cx)),
					),
				)
				.with_priority(3),
			)
	});
	div()
		.id(key)
		.relative()
		.flex_1()
		.mx(rem(-ITEM_INSET_X))
		.my(rem(-ITEM_INSET_Y))
		.px(rem(ITEM_INSET_X))
		.py(rem(ITEM_INSET_Y))
		.rounded(radius)
		.when(open, |content| content.bg(highlight))
		.on_hover(move |hovered, _, cx| {
			row_hover.update(cx, |state, cx| {
				state.row = *hovered;
				cx.notify();
			})
		})
		.child(
			div()
				.flex()
				.items_center()
				.child(div().flex_1().min_w_0().child(row.render(cx)))
				.child(Icon::new(IconKind::ChevronRight).size(rem(16.)).color(chevron)),
		)
		.children(flyout)
		.into_any_element()
}

fn panel(entries: &[MenuEntry], window: &mut Window, cx: &mut App) -> AnyElement {
	let theme = cx.global::<Theme>().clone();
	let mut rows: Vec<AnyElement> = Vec::new();
	collect_rows(entries, &mut rows, window, cx);
	div()
		.flex()
		.flex_col()
		.min_w(rem(MENU_WIDTH))
		.p(rem(PANEL_PADDING))
		.rounded(theme.radius_md)
		.border_1()
		.border_color(theme.divider)
		.bg(theme.background_floating)
		.children(rows)
		.into_any_element()
}

fn collect_rows(entries: &[MenuEntry], rows: &mut Vec<AnyElement>, window: &mut Window, cx: &mut App) {
	for (index, entry) in entries.iter().enumerate() {
		match entry {
			MenuEntry::Separator => {
				let divider = cx.global::<Theme>().divider;
				rows.push(div().my(rem(4.)).h(px(1.)).bg(divider).into_any_element());
			}
			MenuEntry::Lazy(build) => {
				let built = build(cx);
				collect_rows(&built.entries, rows, window, cx);
			}
			MenuEntry::Element { render, .. } => rows.push(render(window, cx)),
			MenuEntry::Submenu { label, icon, entries } => {
				let row = Row {
					label: label.clone(),
					icon: *icon,
					trailing: None,
					danger: false,
					disabled: false,
					checked: false,
				};
				rows.push(
					div()
						.px(rem(ITEM_INSET_X))
						.py(rem(ITEM_INSET_Y))
						.child(submenu_row(&row, entries, window, cx))
						.into_any_element(),
				);
			}
			MenuEntry::Item {
				label,
				icon,
				trailing,
				danger,
				disabled,
				checked,
				on_select,
			} => {
				let row = Row {
					label: label.clone(),
					icon: *icon,
					trailing: *trailing,
					danger: *danger,
					disabled: *disabled,
					checked: checked.unwrap_or(false),
				};
				let theme = cx.global::<Theme>();
				let hover = theme.modifier_selected;
				let radius = theme.radius_sm;
				let on_select = on_select.clone();
				let disabled = *disabled;
				rows.push(
					div()
						.id(("menu-flyout-item", index))
						.px(rem(ITEM_INSET_X))
						.py(rem(ITEM_INSET_Y))
						.rounded(radius)
						.when(!disabled, |item| {
							item.cursor_pointer()
								.hover(move |style| style.bg(hover))
								.on_mouse_down(MouseButton::Left, move |_, window, cx| on_select(window, cx))
						})
						.child(row.render(cx))
						.into_any_element(),
				);
			}
		}
	}
}
