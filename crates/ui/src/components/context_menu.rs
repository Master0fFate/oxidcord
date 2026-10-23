use std::rc::Rc;

use gpui::{App, Context, ParentElement as _, SharedString, Styled as _, Window, div};
use gpui_component::Icon as ComponentIcon;
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::icons::IconKind;
use crate::theme::{Theme, rem};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

pub enum MenuEntry {
	Item {
		label: SharedString,
		icon: Option<IconKind>,
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

	pub fn separator(mut self) -> Self {
		self.entries.push(MenuEntry::Separator);
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
				menu.min_w(rem(188.).to_pixels(window.rem_size())),
				&entries,
				window,
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
			danger,
			disabled: false,
			checked: None,
			on_select: Rc::new(on_select),
		});
		self
	}
}

fn populate(
	menu: PopupMenu,
	entries: &[MenuEntry],
	window: &mut Window,
	cx: &mut Context<PopupMenu>,
) -> PopupMenu {
	entries.iter().fold(menu, |menu, entry| match entry {
		MenuEntry::Separator => menu.separator(),
		MenuEntry::Lazy(build) => {
			let built = build(cx);
			populate(menu, &built.entries, window, cx)
		}
		MenuEntry::Submenu { label, icon, entries } => {
			let entries = entries.clone();
			let icon = icon.map(|kind| ComponentIcon::default().path(kind.path()));
			menu.submenu_with_icon(icon, label.clone(), window, cx, move |submenu, window, cx| {
				populate(
					submenu.min_w(rem(188.).to_pixels(window.rem_size())),
					&entries,
					window,
					cx,
				)
			})
		}
		MenuEntry::Item {
			label,
			icon,
			danger,
			disabled,
			checked,
			on_select,
		} => {
			let on_select = on_select.clone();
			let item = if *danger {
				let label = label.clone();
				PopupMenuItem::element(move |_, cx| {
					div().text_color(cx.global::<Theme>().danger).child(label.clone())
				})
			} else {
				PopupMenuItem::new(label.clone())
			};
			let item = match icon {
				Some(kind) => item.icon(ComponentIcon::default().path(kind.path())),
				None => item,
			}
			.checked(checked.unwrap_or(false))
			.disabled(*disabled);
			menu.item(item.on_click(move |_, window, cx| on_select(window, cx)))
		}
	})
}
