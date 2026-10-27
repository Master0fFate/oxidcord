use std::rc::Rc;

use gpui::{
	AnyElement, App, ClickEvent, ElementId, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};

use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct SectionHeader {
	id: ElementId,
	label: SharedString,
	collapsed: Option<bool>,
	trailing: Option<AnyElement>,
	trailing_on_hover: bool,
	on_click: Option<ClickHandler>,
}

impl SectionHeader {
	pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
		Self {
			id: id.into(),
			label: label.into(),
			collapsed: None,
			trailing: None,
			trailing_on_hover: false,
			on_click: None,
		}
	}

	pub fn collapsible(mut self, collapsed: bool) -> Self {
		self.collapsed = Some(collapsed);
		self
	}

	pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
		self.trailing = Some(trailing.into_any_element());
		self
	}

	pub fn trailing_on_hover(mut self) -> Self {
		self.trailing_on_hover = true;
		self
	}

	pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
		self.on_click = Some(Rc::new(handler));
		self
	}
}

impl RenderOnce for SectionHeader {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let rest = theme.text_muted;
		let hover = theme.interactive_hover;
		let group: SharedString = format!("section-header-{}", self.id).into();
		let chevron = self.collapsed.map(|collapsed| {
			let kind = if collapsed {
				IconKind::ChevronRight
			} else {
				IconKind::ChevronDown
			};
			Icon::new(kind)
				.size(rem(12.))
				.color(rest)
				.group_hover(group.clone(), hover)
		});
		let clickable = self.on_click.is_some();
		let trailing_on_hover = self.trailing_on_hover;
		let hover_group = group.clone();
		div()
			.id(self.id)
			.group(group)
			.flex()
			.items_center()
			.gap(theme.space_4)
			.h(rem(24.))
			.px(theme.space_8)
			.text_size(theme.font_xs)
			.font_weight(FontWeight::MEDIUM)
			.text_color(rest)
			.hover(move |style| style.text_color(hover))
			.when(clickable, |row| row.cursor_pointer())
			.when_some(self.on_click, |row, handler| {
				row.on_click(move |event, window, cx| handler(event, window, cx))
			})
			.child(div().min_w_0().truncate().child(self.label.to_uppercase()))
			.children(chevron)
			.child(div().flex_1())
			.children(self.trailing.map(|trailing| {
				div()
					.flex()
					.items_center()
					.when(trailing_on_hover, |slot| {
						slot.invisible().group_hover(hover_group, |style| style.visible())
					})
					.child(trailing)
			}))
	}
}
