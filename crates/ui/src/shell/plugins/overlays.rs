use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
	Anchor, AnyElement, App, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
	MouseButton, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _, Subscription, Task,
	Window, anchored, deferred, div, px,
};
use plugin_host::api::model::ViewTree;
use plugin_host::mounts::Overlay;
use plugin_host::{Permission, PluginDebug, ViewContext};
use session::Command;
use state::Changes;

use super::bridge::{PluginBridge, Surface};
use super::view::{Ctx, Scope};
use crate::components::IconButton;
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::shell::SettingsBackend;
use crate::theme::{Theme, rem};

const TOAST_LIFETIME: Duration = Duration::from_secs(6);
const DEBUG_REFRESH: Duration = Duration::from_secs(1);
const PENDING_LIFETIME: Duration = Duration::from_secs(120);
const TOAST_WIDTH: f32 = 320.;
const MODAL_MIN_WIDTH: f32 = 320.;
const MODAL_MAX_WIDTH: f32 = 440.;
const POPOVER_MIN_WIDTH: f32 = 240.;
const POPOVER_MAX_WIDTH: f32 = 420.;
const POPOVER_GAP: f32 = 12.;
const WINDOW_MARGIN: f32 = 8.;

pub struct GrantRequest {
	pub id: String,
	pub version: String,
	pub permissions: Vec<Permission>,
	pub hosts: Vec<String>,
}

struct PendingGrant {
	request: GrantRequest,
	since: Instant,
}

struct Toast {
	id: u64,
	plugin: Arc<str>,
	tree: Arc<ViewTree>,
	legacy: bool,
}

struct Modal {
	plugin: Arc<str>,
	title: SharedString,
	tree: Arc<ViewTree>,
}

pub enum PluginUiEvent {
	Insert(String),
	PageChanged,
}

type Shown = Option<(Arc<str>, Arc<ViewTree>)>;
type Page = (Arc<str>, SharedString);

pub struct PluginUi {
	data: Entity<Data>,
	toasts: Vec<Toast>,
	toast_seen: HashMap<Arc<str>, Arc<ViewTree>>,
	modal: Option<Modal>,
	page: Option<Page>,
	next_toast: u64,
	overlays_seen: u64,
	dismissed_modal: Shown,
	dismissed_popover: Shown,
	anchor: Option<Point<Pixels>>,
	right_open: bool,
	pending: Vec<PendingGrant>,
	debug_visible: bool,
	debug: Vec<PluginDebug>,
	debug_task: Option<Task<()>>,
	_subscription: Subscription,
}

impl PluginUi {
	pub fn new(data: Entity<Data>, backend: &Rc<SettingsBackend>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |this, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event {
				this.on_changes(changes, cx);
			}
		});
		let mut ui = Self {
			data,
			toasts: Vec::new(),
			toast_seen: HashMap::new(),
			modal: None,
			page: None,
			next_toast: 0,
			overlays_seen: 0,
			dismissed_modal: None,
			dismissed_popover: None,
			anchor: None,
			right_open: false,
			pending: Vec::new(),
			debug_visible: false,
			debug: Vec::new(),
			debug_task: None,
			_subscription: subscription,
		};
		ui.set_debug_visible((backend.read)().developer.show_plugin_debug, cx);
		ui
	}

	pub fn right_open(&self) -> bool {
		self.right_open
	}

	pub fn page(&self) -> Option<&Page> {
		self.page.as_ref()
	}

	pub fn toggle_page(&mut self, target: Page, cx: &mut Context<Self>) {
		self.page = if self.page.as_ref() == Some(&target) {
			None
		} else {
			Some(target)
		};
		cx.emit(PluginUiEvent::PageChanged);
		cx.notify();
	}

	pub fn close_page(&mut self, cx: &mut Context<Self>) {
		if self.page.take().is_some() {
			cx.emit(PluginUiEvent::PageChanged);
			cx.notify();
		}
	}

	pub fn toggle_right(&mut self, cx: &mut Context<Self>) {
		self.right_open = !self.right_open;
		cx.notify();
	}

	pub fn close_right(&mut self, cx: &mut Context<Self>) {
		if self.right_open {
			self.right_open = false;
			cx.notify();
		}
	}

	pub fn remember_anchor(&mut self, at: Point<Pixels>) {
		self.anchor = Some(at);
	}

	pub fn debug_visible(&self) -> bool {
		self.debug_visible
	}

	pub fn debug(&self) -> &[PluginDebug] {
		&self.debug
	}

	pub fn set_debug_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
		self.debug_visible = visible;
		if visible {
			self.start_debug(cx);
		} else {
			self.debug_task = None;
		}
		cx.notify();
	}

	fn start_debug(&mut self, cx: &mut Context<Self>) {
		if self.debug_task.is_some() {
			return;
		}
		self.debug_task = Some(cx.spawn(async move |this, cx| {
			loop {
				if this.update(cx, |this, cx| this.refresh_debug(cx)).is_err() {
					break;
				}
				cx.background_executor().timer(DEBUG_REFRESH).await;
			}
		}));
	}

	fn refresh_debug(&mut self, cx: &mut Context<Self>) {
		let snapshot = self.data.read(cx).session().plugins().debug_snapshot();
		if snapshot != self.debug {
			self.debug = snapshot;
			cx.notify();
		}
	}

	pub fn queue_grant(&mut self, request: GrantRequest) {
		self.pending.push(PendingGrant {
			request,
			since: Instant::now(),
		});
	}

	pub fn action_sent(&mut self, plugin: &str, cx: &mut Context<Self>) {
		let mut changed = false;
		for shown in [&mut self.dismissed_modal, &mut self.dismissed_popover] {
			if shown.as_ref().is_some_and(|(owner, _)| &**owner == plugin) {
				*shown = None;
				changed = true;
			}
		}
		if changed {
			cx.notify();
		}
	}

	pub fn dismiss_top(&mut self, cx: &mut Context<Self>) -> bool {
		if self.modal.take().is_some() {
			cx.notify();
			return true;
		}
		if let Some(entry) = self.visible("modal", &self.dismissed_modal, cx) {
			self.dismissed_modal = Some((entry.plugin_id, entry.tree));
		} else if let Some(entry) = self.visible("popover", &self.dismissed_popover, cx) {
			self.dismissed_popover = Some((entry.plugin_id, entry.tree));
		} else {
			return false;
		}
		cx.notify();
		true
	}

	fn dismiss_popover(&mut self, cx: &mut Context<Self>) {
		if let Some(entry) = self.visible("popover", &self.dismissed_popover, cx) {
			self.dismissed_popover = Some((entry.plugin_id, entry.tree));
			cx.notify();
		}
	}

	fn dismiss_toast(&mut self, id: u64, cx: &mut Context<Self>) {
		self.toasts.retain(|toast| toast.id != id);
		cx.notify();
	}

	fn visible(&self, slot: &str, dismissed: &Shown, cx: &App) -> Option<Surface> {
		cx.global::<PluginBridge>()
			.slot_entries(slot, cx)
			.iter()
			.find(|entry| {
				dismissed
					.as_ref()
					.is_none_or(|(owner, tree)| *owner != entry.plugin_id || *tree != entry.tree)
			})
			.cloned()
	}

	fn on_changes(&mut self, changes: &Changes, cx: &mut Context<Self>) {
		if changes.plugins {
			cx.global::<PluginBridge>().forget_names();
			self.process_pending(cx);
		}
		if changes.plugin_slots {
			self.sync_toasts(cx);
			self.sync_dismissed(cx);
			self.drain_overlays(cx);
			self.sync_page(cx);
		}
		if changes.plugins || changes.plugin_slots {
			cx.notify();
		}
	}

	fn sync_page(&mut self, cx: &mut Context<Self>) {
		let Some((plugin, key)) = &self.page else {
			return;
		};
		let present = cx
			.global::<PluginBridge>()
			.mounts("main-view", cx)
			.iter()
			.any(|surface| surface.plugin_id == *plugin && surface.key == *key);
		if !present {
			self.close_page(cx);
		}
	}

	fn drain_overlays(&mut self, cx: &mut Context<Self>) {
		let mounts = self.data.read(cx).session().plugin_mounts();
		let revision = mounts.overlays_revision();
		if revision == self.overlays_seen {
			return;
		}
		self.overlays_seen = revision;
		let overlays = mounts.take_overlays();
		for overlay in overlays {
			match overlay {
				Overlay::Toast { plugin_id, tree, ttl } => {
					self.push_toast(plugin_id, Arc::new(tree), false, ttl, cx)
				}
				Overlay::Modal {
					plugin_id,
					title: Some(title),
					tree,
				} => {
					self.modal = Some(Modal {
						plugin: plugin_id,
						title: title.into(),
						tree: Arc::new(tree),
					});
				}
				Overlay::Modal {
					plugin_id,
					title: None,
					tree,
				} => {
					if let Some(modal) = self.modal.as_mut().filter(|modal| modal.plugin == plugin_id) {
						modal.tree = Arc::new(tree);
					}
				}
				Overlay::Close { plugin_id } => {
					if self.modal.as_ref().is_some_and(|modal| modal.plugin == plugin_id) {
						self.modal = None;
					}
				}
				Overlay::Insert { text, .. } => cx.emit(PluginUiEvent::Insert(text)),
			}
		}
	}

	fn push_toast(
		&mut self,
		plugin: Arc<str>,
		tree: Arc<ViewTree>,
		legacy: bool,
		ttl: Duration,
		cx: &mut Context<Self>,
	) {
		let id = self.next_toast;
		self.next_toast += 1;
		self.toasts.push(Toast {
			id,
			plugin,
			tree,
			legacy,
		});
		cx.spawn(async move |this, cx| {
			cx.background_executor().timer(ttl).await;
			this.update(cx, |this, cx| this.dismiss_toast(id, cx)).ok();
		})
		.detach();
	}

	fn sync_dismissed(&mut self, cx: &mut Context<Self>) {
		let bridge = cx.global::<PluginBridge>();
		for (slot, shown) in [
			("modal", &mut self.dismissed_modal),
			("popover", &mut self.dismissed_popover),
		] {
			let entries = bridge.slot_entries(slot, cx);
			if shown
				.as_ref()
				.is_some_and(|(owner, _)| !entries.iter().any(|entry| entry.plugin_id == *owner))
			{
				*shown = None;
			}
		}
	}

	fn sync_toasts(&mut self, cx: &mut Context<Self>) {
		let entries = cx.global::<PluginBridge>().slot_entries("toast", cx);
		self.toast_seen
			.retain(|plugin, _| entries.iter().any(|entry| entry.plugin_id == *plugin));
		for entry in entries.iter() {
			if self
				.toast_seen
				.get(&entry.plugin_id)
				.is_some_and(|seen| *seen == entry.tree)
			{
				continue;
			}
			self.toast_seen
				.insert(Arc::clone(&entry.plugin_id), Arc::clone(&entry.tree));
			self.push_toast(
				Arc::clone(&entry.plugin_id),
				Arc::clone(&entry.tree),
				true,
				TOAST_LIFETIME,
				cx,
			);
		}
	}

	fn process_pending(&mut self, cx: &mut Context<Self>) {
		if self.pending.is_empty() {
			return;
		}
		let (list, failed) = {
			let session = self.data.read(cx).session();
			(session.plugin_list(), session.plugin_error().is_some())
		};
		let mut waiting = Vec::new();
		for pending in std::mem::take(&mut self.pending) {
			let request = &pending.request;
			let present = list
				.iter()
				.any(|info| info.id == request.id && info.version.to_string() == request.version);
			if present {
				self.data.read(cx).send(Command::PluginGrant {
					id: request.id.clone(),
					permissions: request.permissions.clone(),
					hosts: request.hosts.clone(),
				});
			} else if !failed && pending.since.elapsed() < PENDING_LIFETIME {
				waiting.push(pending);
			}
		}
		self.pending = waiting;
	}

	fn panel(theme: &Theme, name: SharedString) -> gpui::Div {
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.p(theme.space_16)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.text_color(theme.text_normal)
			.child(
				div()
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_secondary)
					.child(name.to_uppercase()),
			)
	}

	fn modal_layer(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let (name, body) = match &self.modal {
			Some(modal) => {
				let scope = Scope::mount(&modal.plugin, "modal", "", ViewContext::default());
				(modal.title.clone(), Ctx::new(&modal.tree, scope).column(theme))
			}
			None => {
				let entry = self.visible("modal", &self.dismissed_modal, cx)?;
				let name = cx.global::<PluginBridge>().name_of(&entry.plugin_id, cx);
				let scope = Scope::slot(&entry.plugin_id, "modal", "");
				(name, Ctx::new(&entry.tree, scope).column(theme))
			}
		};
		Some(
			div()
				.absolute()
				.top_0()
				.left_0()
				.size_full()
				.flex()
				.items_center()
				.justify_center()
				.bg(theme.overlay)
				.occlude()
				.on_mouse_down(
					MouseButton::Left,
					cx.listener(|this, _, _, cx| {
						this.dismiss_top(cx);
					}),
				)
				.child(
					Self::panel(theme, name)
						.min_w(rem(MODAL_MIN_WIDTH))
						.max_w(rem(MODAL_MAX_WIDTH))
						.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
						.child(body),
				)
				.into_any_element(),
		)
	}

	fn popover_layer(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let entry = self.visible("popover", &self.dismissed_popover, cx)?;
		let at = self.anchor?;
		let name = cx.global::<PluginBridge>().name_of(&entry.plugin_id, cx);
		let scope = Scope::slot(&entry.plugin_id, "popover", "");
		let body = Ctx::new(&entry.tree, scope).column(theme);
		Some(
			deferred(
				anchored()
					.position(at - gpui::point(px(0.), theme.pixels(rem(POPOVER_GAP))))
					.anchor(Anchor::BottomRight)
					.snap_to_window_with_margin(theme.pixels(rem(WINDOW_MARGIN)))
					.child(
						div()
							.occlude()
							.on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_popover(cx)))
							.child(
								Self::panel(theme, name)
									.min_w(rem(POPOVER_MIN_WIDTH))
									.max_w(rem(POPOVER_MAX_WIDTH))
									.child(body),
							),
					),
			)
			.with_priority(2)
			.into_any_element(),
		)
	}

	fn toast_layer(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		if self.toasts.is_empty() {
			return None;
		}
		let cards: Vec<AnyElement> = self
			.toasts
			.iter()
			.map(|toast| {
				let id = toast.id;
				let name = cx.global::<PluginBridge>().name_of(&toast.plugin, cx);
				let scope = if toast.legacy {
					Scope::slot(&toast.plugin, "toast", "")
				} else {
					Scope::mount(&toast.plugin, "toast", "", ViewContext::default())
				};
				let body = Ctx::new(&toast.tree, scope).column(theme);
				Self::panel(theme, name)
					.w(rem(TOAST_WIDTH))
					.relative()
					.child(
						div().absolute().top(theme.space_8).right(theme.space_8).child(
							IconButton::new(("plugin-toast-close", id), IconKind::Close)
								.icon_size(rem(16.))
								.hit_size(rem(24.))
								.tooltip(cx.t("plugin-toast-dismiss"))
								.on_click(cx.listener(move |this, _, _, cx| this.dismiss_toast(id, cx))),
						),
					)
					.child(body)
					.into_any_element()
			})
			.collect();
		Some(
			div()
				.absolute()
				.right(theme.space_16)
				.bottom(theme.space_16)
				.flex()
				.flex_col()
				.gap(theme.space_8)
				.children(cards)
				.into_any_element(),
		)
	}
}

impl EventEmitter<PluginUiEvent> for PluginUi {}

impl Render for PluginUi {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		div()
			.absolute()
			.top_0()
			.left_0()
			.size_full()
			.children(self.toast_layer(&theme, cx))
			.children(self.popover_layer(&theme, cx))
			.children(self.modal_layer(&theme, cx))
	}
}
