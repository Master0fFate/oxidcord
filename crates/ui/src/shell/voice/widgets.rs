use std::rc::Rc;

use gpui::{
	AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement,
	MouseButton, MouseDownEvent, ParentElement as _, Rems, RenderOnce, SharedString,
	StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Selectable;
use state::VideoKind;

use super::participants::Participant;
use crate::components::{Avatar, tooltip};
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

const PING_GOOD_MS: u32 = 100;
const PING_FAIR_MS: u32 = 250;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

pub fn speaking_avatar(participant: &Participant, size: Rems, theme: &Theme) -> impl IntoElement {
	div()
		.relative()
		.flex_none()
		.size(size)
		.child(
			Avatar::new(participant.name.clone())
				.size(size)
				.url(participant.avatar.clone()),
		)
		.when(participant.speaking, |avatar| {
			avatar.child(
				div()
					.absolute()
					.top(-theme.space_4)
					.left(-theme.space_4)
					.size(size + theme.space_4 * 2.)
					.rounded(theme.round(size + theme.space_4 * 2.))
					.border_2()
					.border_color(theme.speaking_ring),
			)
		})
}

pub fn state_icons(participant: &Participant, size: Rems, theme: &Theme) -> Vec<AnyElement> {
	let mut icons = Vec::new();
	let mut push = |kind: IconKind, color: Hsla| {
		icons.push(Icon::new(kind).size(size).color(color).into_any_element());
	};
	match participant.video {
		Some(VideoKind::Camera) => push(IconKind::Video, theme.text_muted),
		Some(VideoKind::Screen) => push(IconKind::ScreenShare, theme.text_muted),
		None => {}
	}
	if participant.deafened {
		push(IconKind::HeadphonesOff, theme.danger);
	} else if participant.muted {
		push(IconKind::MicOff, theme.danger);
	}
	icons
}

pub fn live_badge(label: SharedString, theme: &Theme) -> impl IntoElement {
	div()
		.flex_none()
		.px(theme.space_4)
		.rounded(theme.radius_sm)
		.bg(theme.live_badge)
		.text_size(theme.font_tiny)
		.line_height(theme.voice_icon_sm)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.text_on_brand)
		.child(label)
}

pub fn participant_row(
	person: &Participant,
	theme: &Theme,
	live_label: SharedString,
	on_context: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
	div()
		.id(("voice-member", person.user.get()))
		.flex()
		.items_center()
		.gap(theme.space_8)
		.h(theme.voice_row_height)
		.ml(theme.space_32 - theme.space_4)
		.mr(theme.space_8)
		.px(theme.space_8)
		.rounded(theme.radius_sm)
		.hover(|style| style.bg(theme.modifier_hover))
		.on_mouse_down(MouseButton::Right, on_context)
		.child(speaking_avatar(person, theme.voice_row_avatar, theme))
		.child(
			div()
				.flex_1()
				.min_w_0()
				.truncate()
				.text_size(theme.font_sm)
				.text_color(if person.speaking {
					theme.header_primary
				} else {
					theme.text_muted
				})
				.child(person.name.clone()),
		)
		.when(person.live, |row| row.child(live_badge(live_label, theme)))
		.children(state_icons(person, theme.voice_badge_icon, theme))
		.into_any_element()
}

pub fn signal(ping: Option<u32>, theme: &Theme) -> (IconKind, Hsla) {
	match ping {
		Some(ms) if ms < PING_GOOD_MS => (IconKind::SignalHigh, theme.success),
		Some(ms) if ms < PING_FAIR_MS => (IconKind::SignalMedium, theme.warning),
		Some(_) => (IconKind::SignalLow, theme.danger),
		None => (IconKind::SignalHigh, theme.text_muted),
	}
}

#[derive(IntoElement)]
pub struct ControlButton {
	id: ElementId,
	icon: IconKind,
	tooltip: SharedString,
	highlight: bool,
	alert: bool,
	danger: bool,
	on_click: Option<ClickHandler>,
}

impl ControlButton {
	pub fn new(id: impl Into<ElementId>, icon: IconKind, tooltip: impl Into<SharedString>) -> Self {
		Self {
			id: id.into(),
			icon,
			tooltip: tooltip.into(),
			highlight: false,
			alert: false,
			danger: false,
			on_click: None,
		}
	}

	pub fn highlight(mut self, highlight: bool) -> Self {
		self.highlight = highlight;
		self
	}

	pub fn alert(mut self, alert: bool) -> Self {
		self.alert = alert;
		self
	}

	pub fn danger(mut self) -> Self {
		self.danger = true;
		self
	}

	pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
		self.on_click = Some(Rc::new(handler));
		self
	}
}

impl RenderOnce for ControlButton {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let (background, hover, icon_color) = if self.danger {
			(theme.danger, theme.danger_hover, theme.text_on_brand)
		} else if self.highlight {
			(
				theme.interactive_active,
				theme.interactive_hover,
				theme.background_tertiary,
			)
		} else if self.alert {
			(theme.button_secondary, theme.button_secondary_hover, theme.danger)
		} else {
			(
				theme.button_secondary,
				theme.button_secondary_hover,
				theme.text_on_brand,
			)
		};
		div()
			.id(self.id)
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.voice_control)
			.rounded_full()
			.bg(background)
			.cursor_pointer()
			.hover(move |style| style.bg(hover))
			.tooltip(tooltip(self.tooltip))
			.when_some(self.on_click, |button, handler| {
				button.on_click(move |event, window, cx| handler(event, window, cx))
			})
			.child(Icon::new(self.icon).size(theme.voice_icon_md).color(icon_color))
	}
}

#[derive(IntoElement)]
pub struct SignalTrigger {
	icon: IconKind,
	color: Hsla,
	selected: bool,
}

impl SignalTrigger {
	pub fn new(icon: IconKind, color: Hsla) -> Self {
		Self {
			icon,
			color,
			selected: false,
		}
	}
}

impl Selectable for SignalTrigger {
	fn selected(mut self, selected: bool) -> Self {
		self.selected = selected;
		self
	}

	fn is_selected(&self) -> bool {
		self.selected
	}
}

impl RenderOnce for SignalTrigger {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		div()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.voice_button)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.when(self.selected, |trigger| trigger.bg(theme.modifier_selected))
			.hover(|style| style.bg(theme.modifier_hover))
			.child(Icon::new(self.icon).size(theme.voice_icon_md).color(self.color))
	}
}
