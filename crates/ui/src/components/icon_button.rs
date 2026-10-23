use std::rc::Rc;

use gpui::{
	App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Rems,
	RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};

use super::tooltip::tooltip;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct IconButton {
	id: ElementId,
	icon: IconKind,
	icon_size: Rems,
	hit_size: Rems,
	tooltip: Option<SharedString>,
	active: bool,
	filled_hover: bool,
	color: Option<Hsla>,
	on_click: Option<ClickHandler>,
}

impl IconButton {
	pub fn new(id: impl Into<ElementId>, icon: IconKind) -> Self {
		Self {
			id: id.into(),
			icon,
			icon_size: rem(20.),
			hit_size: rem(28.),
			tooltip: None,
			active: false,
			filled_hover: false,
			color: None,
			on_click: None,
		}
	}

	pub fn icon_size(mut self, size: Rems) -> Self {
		self.icon_size = size;
		self
	}

	pub fn hit_size(mut self, size: Rems) -> Self {
		self.hit_size = size;
		self
	}

	pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
		self.tooltip = Some(text.into());
		self
	}

	pub fn active(mut self, active: bool) -> Self {
		self.active = active;
		self
	}

	pub fn filled_hover(mut self) -> Self {
		self.filled_hover = true;
		self
	}

	pub fn color(mut self, color: Hsla) -> Self {
		self.color = Some(color);
		self
	}

	pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
		self.on_click = Some(Rc::new(handler));
		self
	}
}

impl RenderOnce for IconButton {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let group: SharedString = format!("icon-button-{}", self.id).into();
		let (shown, hovered) = match (self.active, self.color) {
			(true, _) => (theme.interactive_active, theme.interactive_active),
			(false, Some(color)) => (color, color),
			(false, None) => (theme.interactive_normal, theme.interactive_hover),
		};
		let hover_background = theme.modifier_hover;
		let active_background = theme.modifier_selected;
		let radius = theme.radius_sm;
		let filled_hover = self.filled_hover;
		let active = self.active;
		let icon = Icon::new(self.icon)
			.size(self.icon_size)
			.color(shown)
			.group_hover(group.clone(), hovered);
		div()
			.id(self.id)
			.group(group)
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(self.hit_size)
			.rounded(radius)
			.cursor_pointer()
			.when(active && filled_hover, |button| button.bg(active_background))
			.when(filled_hover && !active, |button| {
				button.hover(move |style| style.bg(hover_background))
			})
			.when_some(self.tooltip, |button, text| button.tooltip(tooltip(text)))
			.when_some(self.on_click, |button, handler| {
				button.on_click(move |event, window, cx| handler(event, window, cx))
			})
			.child(icon)
	}
}
