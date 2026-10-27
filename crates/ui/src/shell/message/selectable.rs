use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
	App, BorderStyle, Bounds, Corners, CursorStyle, DispatchPhase, Edges, Element, ElementId,
	GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseDownEvent,
	MouseUpEvent, PaintQuad, Pixels, Point, SharedString, StyledText, TextLayout, Window, transparent_black,
};
use gpui_base::{TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};

type ClickHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct RunState {
	handle: TextSelectionHandle,
	pressed: Rc<Cell<Option<usize>>>,
}

pub struct SelectableRun {
	id: ElementId,
	text: SharedString,
	styled: StyledText,
	order: u64,
	selection: Hsla,
	clicks: Vec<Range<usize>>,
	on_click: Option<ClickHandler>,
}

impl SelectableRun {
	pub fn new(id: ElementId, text: SharedString, styled: StyledText, order: u64, selection: Hsla) -> Self {
		Self {
			id,
			text,
			styled,
			order,
			selection,
			clicks: Vec::new(),
			on_click: None,
		}
	}

	pub fn on_click(
		mut self,
		ranges: Vec<Range<usize>>,
		handler: impl Fn(usize, &mut Window, &mut App) + 'static,
	) -> Self {
		self.clicks = ranges;
		self.on_click = Some(Rc::new(handler));
		self
	}

	fn click_at(&self, layout: &TextLayout, position: Point<Pixels>) -> Option<usize> {
		let index = layout.index_for_position(position).ok()?;
		self.clicks.iter().position(|range| range.contains(&index))
	}
}

fn line_quads(
	start: Point<Pixels>,
	end: Point<Pixels>,
	bounds: Bounds<Pixels>,
	line: Pixels,
) -> Vec<Bounds<Pixels>> {
	if start.y == end.y {
		return vec![Bounds::from_corners(start, Point::new(end.x, end.y + line))];
	}
	let mut quads = vec![Bounds::from_corners(
		start,
		Point::new(bounds.right(), start.y + line),
	)];
	if end.y > start.y + line {
		quads.push(Bounds::from_corners(
			Point::new(bounds.left(), start.y + line),
			Point::new(bounds.right(), end.y),
		));
	}
	quads.push(Bounds::from_corners(
		Point::new(bounds.left(), end.y),
		Point::new(end.x, end.y + line),
	));
	quads
}

fn paint_highlight(layout: &TextLayout, range: Range<usize>, color: Hsla, window: &mut Window) {
	let (Some(start), Some(end)) = (
		layout.position_for_index(range.start),
		layout.position_for_index(range.end),
	) else {
		return;
	};
	for bounds in line_quads(start, end, layout.bounds(), layout.line_height()) {
		window.paint_quad(PaintQuad {
			bounds,
			background: color.into(),
			corner_radii: Corners::default(),
			border_widths: Edges::default(),
			border_color: transparent_black(),
			border_style: BorderStyle::default(),
		});
	}
}

impl IntoElement for SelectableRun {
	type Element = Self;

	fn into_element(self) -> Self::Element {
		self
	}
}

impl Element for SelectableRun {
	type RequestLayoutState = RunState;
	type PrepaintState = Hitbox;

	fn id(&self) -> Option<ElementId> {
		Some(self.id.clone())
	}

	fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		global_id: Option<&GlobalElementId>,
		inspector_id: Option<&InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (LayoutId, Self::RequestLayoutState) {
		let state = window.with_element_state(
			global_id.expect("selectable runs have an element id"),
			|retained: Option<RunState>, _| {
				let state = retained.unwrap_or_else(|| RunState {
					handle: TextSelectionHandle::new(self.text.clone(), cx),
					pressed: Rc::default(),
				});
				(state.clone(), state)
			},
		);
		let (layout, ()) = self.styled.request_layout(global_id, inspector_id, window, cx);
		(layout, state)
	}

	fn prepaint(
		&mut self,
		global_id: Option<&GlobalElementId>,
		inspector_id: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		state: &mut Self::RequestLayoutState,
		window: &mut Window,
		cx: &mut App,
	) -> Self::PrepaintState {
		self.styled
			.prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
		let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
		let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
			.with_document_order(self.order)
			.with_text_bounds(vec![bounds])
			.with_rendered_element(&state.handle, window, cx);
		state.handle.register(registration, window, cx);
		hitbox
	}

	fn paint(
		&mut self,
		global_id: Option<&GlobalElementId>,
		inspector_id: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		state: &mut Self::RequestLayoutState,
		hitbox: &mut Self::PrepaintState,
		window: &mut Window,
		cx: &mut App,
	) {
		let layout = self.styled.layout().clone();
		let projection = state.handle.update_runs(
			&[TextSelectionRun::new(self.text.clone(), layout.clone(), bounds)
				.with_document_order(self.order)],
			cx,
		);
		for range in projection.ranges().iter().flatten().cloned() {
			paint_highlight(&layout, range, self.selection, window);
		}
		self.styled
			.paint(global_id, inspector_id, bounds, &mut (), &mut (), window, cx);
		let pointing = hitbox.is_hovered(window) && self.click_at(&layout, window.mouse_position()).is_some();
		window.set_cursor_style(
			if pointing {
				CursorStyle::PointingHand
			} else {
				CursorStyle::IBeam
			},
			hitbox,
		);
		let Some(on_click) = self.on_click.take() else {
			return;
		};
		let clicks = std::mem::take(&mut self.clicks);
		let pressed = state.pressed.clone();
		let down_hitbox = hitbox.clone();
		let down_layout = layout.clone();
		let down_clicks = clicks.clone();
		window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
			if phase == DispatchPhase::Bubble && down_hitbox.is_hovered(window) {
				let index = down_layout.index_for_position(event.position).ok();
				pressed
					.set(index.and_then(|index| down_clicks.iter().position(|range| range.contains(&index))));
			}
		});
		let pressed = state.pressed.clone();
		let up_hitbox = hitbox.clone();
		window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
			if phase != DispatchPhase::Bubble {
				return;
			}
			let Some(target) = pressed.take() else {
				return;
			};
			let released = up_hitbox.is_hovered(window)
				&& layout
					.index_for_position(event.position)
					.ok()
					.is_some_and(|index| clicks[target].contains(&index));
			if released && !TextSelection::has_selection(window, cx) {
				on_click(target, window, cx);
			}
		});
	}
}
