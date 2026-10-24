use std::rc::Rc;
use std::sync::Arc;

use gpui::{
	AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
	SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use plugin_host::ViewContext;

use super::bridge::{PluginBridge, Surface};
use super::icon::PluginIcon;
use super::view::{COMPOSER, Compact, Ctx, STATUS, Scope, TITLE};
use crate::components::{IconButton, SectionHeader, tooltip};
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::theme::{Theme, rem};

const STATUS_BAR_HEIGHT: f32 = 24.;
const SIDEBAR_PANEL_MAX_HEIGHT: f32 = 260.;
const DEBUG_PANEL_MAX_HEIGHT: f32 = 220.;
const DEBUG_ROW_HEIGHT: f32 = 24.;
const DEBUG_NUMBER_WIDTH: f32 = 84.;
const RAIL_ICON: f32 = 24.;
const RAIL_TILE: f32 = 48.;

pub fn scope_of(
	surface: &Surface,
	point: &'static str,
	payload: SharedString,
	context: ViewContext,
) -> Scope {
	if surface.legacy {
		Scope::slot(&surface.plugin_id, point, payload)
	} else {
		Scope::mount(&surface.plugin_id, point, surface.key.clone(), context)
	}
}

pub fn compact_row(
	point: &'static str,
	compact: Compact,
	payload: SharedString,
	context: ViewContext,
	cx: &App,
) -> Vec<AnyElement> {
	let theme = cx.global::<Theme>();
	cx.global::<PluginBridge>()
		.surfaces(point, cx)
		.iter()
		.filter(|surface| !surface.tree.roots.is_empty())
		.map(|surface| {
			let scope = scope_of(surface, point, payload.clone(), context).compact(Some(compact));
			Ctx::new(&surface.tree, scope).row(theme)
		})
		.collect()
}

pub fn composer_toolbar(context: ViewContext, cx: &App) -> Vec<AnyElement> {
	compact_row("composer-toolbar", COMPOSER, SharedString::default(), context, cx)
}

pub fn title_items(cx: &App) -> Vec<AnyElement> {
	if !cx.has_global::<PluginBridge>() {
		return Vec::new();
	}
	compact_row(
		"title-bar",
		TITLE,
		SharedString::default(),
		ViewContext::default(),
		cx,
	)
}

pub fn contextual(point: &'static str, context: ViewContext, cx: &App) -> Vec<AnyElement> {
	if !cx.has_global::<PluginBridge>() {
		return Vec::new();
	}
	let theme = cx.global::<Theme>();
	cx.global::<PluginBridge>()
		.contextual(point, context, cx)
		.iter()
		.map(|surface| {
			Ctx::new(
				&surface.tree,
				scope_of(surface, point, SharedString::default(), context),
			)
			.column(theme)
		})
		.collect()
}

pub fn status_bar(cx: &App) -> Option<AnyElement> {
	let items = compact_row(
		"status-bar",
		STATUS,
		SharedString::default(),
		ViewContext::default(),
		cx,
	);
	if items.is_empty() {
		return None;
	}
	let theme = cx.global::<Theme>();
	Some(
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_16)
			.h(rem(STATUS_BAR_HEIGHT))
			.px(theme.space_16)
			.bg(theme.background_secondary)
			.border_t_1()
			.border_color(theme.divider)
			.text_size(theme.font_xs)
			.text_color(theme.text_muted)
			.children(items)
			.into_any_element(),
	)
}

pub fn panel_toggle(cx: &App) -> Option<AnyElement> {
	let bridge = cx.global::<PluginBridge>();
	if bridge.surfaces("right-panel", cx).is_empty() {
		return None;
	}
	let open = bridge.ui().read(cx).right_open();
	Some(
		IconButton::new("top-plugin-panel", IconKind::Plugins)
			.tooltip(cx.t("plugin-panel-toggle"))
			.active(open)
			.on_click(|_, _, cx| {
				let ui = cx.global::<PluginBridge>().ui().clone();
				ui.update(cx, |ui, cx| ui.toggle_right(cx));
			})
			.into_any_element(),
	)
}

pub fn right_panel_visible(cx: &App) -> bool {
	let bridge = cx.global::<PluginBridge>();
	bridge.ui().read(cx).right_open() && !bridge.surfaces("right-panel", cx).is_empty()
}

pub fn open_page(cx: &App) -> Option<(Arc<str>, SharedString)> {
	cx.try_global::<PluginBridge>()?.ui().read(cx).page().cloned()
}

pub fn app_nav(active: Option<&(Arc<str>, SharedString)>, cx: &App) -> Vec<AnyElement> {
	if !cx.has_global::<PluginBridge>() {
		return Vec::new();
	}
	let theme = cx.global::<Theme>();
	cx.global::<PluginBridge>()
		.mounts("app-nav", cx)
		.iter()
		.enumerate()
		.map(|(index, surface)| {
			let selected =
				active.is_some_and(|(plugin, key)| *plugin == surface.plugin_id && *key == surface.key);
			let target = (Arc::clone(&surface.plugin_id), surface.key.clone());
			let hover = theme.brand;
			div()
				.id(("plugin-app-nav", index))
				.flex()
				.items_center()
				.justify_center()
				.size(rem(RAIL_TILE))
				.cursor_pointer()
				.when(selected, |tile| tile.rounded(theme.radius_lg).bg(theme.brand))
				.when(!selected, |tile| {
					tile.rounded_full()
						.bg(theme.background_primary)
						.hover(move |style| style.rounded(theme.radius_lg).bg(hover))
				})
				.tooltip(tooltip(surface.title.clone()))
				.on_click(move |_, _, cx| {
					let ui = cx.global::<PluginBridge>().ui().clone();
					let target = target.clone();
					ui.update(cx, |ui, cx| ui.toggle_page(target, cx));
				})
				.child(
					PluginIcon::new(surface.icon.clone().unwrap_or_else(|| "puzzle".into()))
						.size(rem(RAIL_ICON))
						.color(if selected {
							theme.text_on_brand
						} else {
							theme.interactive_normal
						}),
				)
				.into_any_element()
		})
		.collect()
}

pub fn main_view(target: &(Arc<str>, SharedString), cx: &App) -> Option<AnyElement> {
	let theme = cx.global::<Theme>();
	let surfaces = cx.global::<PluginBridge>().mounts("main-view", cx);
	let surface = surfaces
		.iter()
		.find(|surface| surface.plugin_id == target.0 && surface.key == target.1)?;
	let scope = scope_of(
		surface,
		"main-view",
		SharedString::default(),
		ViewContext::default(),
	);
	Some(
		div()
			.id("plugin-main-view")
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.children(Ctx::new(&surface.tree, scope).roots(false, theme))
			.into_any_element(),
	)
}

pub fn settings_mounts(plugin: &str, cx: &App) -> Vec<AnyElement> {
	let theme = cx.global::<Theme>();
	cx.global::<PluginBridge>()
		.mounts("settings-page", cx)
		.iter()
		.filter(|surface| &*surface.plugin_id == plugin)
		.map(|surface| {
			let scope = scope_of(
				surface,
				"settings-page",
				SharedString::default(),
				ViewContext::default(),
			);
			Ctx::new(&surface.tree, scope).column(theme)
		})
		.collect()
}

fn panel_body(surface: &Surface, point: &'static str, theme: &Theme) -> AnyElement {
	let scope = scope_of(surface, point, SharedString::default(), ViewContext::default());
	Ctx::new(&surface.tree, scope).column(theme)
}

fn surface_name(surface: &Surface, cx: &App) -> SharedString {
	if surface.title.is_empty() {
		cx.global::<PluginBridge>().name_of(&surface.plugin_id, cx)
	} else {
		surface.title.clone()
	}
}

fn surfaces(point: &str, cx: &App) -> Rc<Vec<Surface>> {
	cx.global::<PluginBridge>().surfaces(point, cx)
}

fn section_label(name: SharedString, theme: &Theme) -> impl IntoElement {
	div()
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_secondary)
		.child(name.to_uppercase())
}

#[derive(IntoElement)]
pub struct SidebarPanel;

impl RenderOnce for SidebarPanel {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let entries = surfaces("sidebar-panel", cx);
		if entries.is_empty() {
			return div().into_any_element();
		}
		let theme = cx.global::<Theme>().clone();
		let sections: Vec<AnyElement> = entries
			.iter()
			.map(|entry| {
				let key = SharedString::from(format!("plugin-sidebar:{}:{}", entry.plugin_id, entry.key));
				let collapsed = window.use_keyed_state(key.clone(), cx, |_, _| false);
				let is_collapsed = *collapsed.read(cx);
				let name = surface_name(entry, cx);
				let header =
					SectionHeader::new(key, name)
						.collapsible(is_collapsed)
						.on_click(move |_, _, cx| {
							collapsed.update(cx, |collapsed, cx| {
								*collapsed = !*collapsed;
								cx.notify();
							})
						});
				div()
					.flex()
					.flex_col()
					.child(header)
					.when_some(
						(!is_collapsed).then(|| panel_body(entry, "sidebar-panel", &theme)),
						|section, body| section.child(div().px(theme.space_8).pb(theme.space_8).child(body)),
					)
					.into_any_element()
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.flex_none()
			.max_h(rem(SIDEBAR_PANEL_MAX_HEIGHT))
			.overflow_hidden()
			.border_t_1()
			.border_color(theme.background_tertiary)
			.bg(theme.background_secondary)
			.children(sections)
			.into_any_element()
	}
}

#[derive(IntoElement)]
pub struct RightPanel;

impl RenderOnce for RightPanel {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let sections: Vec<AnyElement> = surfaces("right-panel", cx)
			.iter()
			.map(|entry| {
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(section_label(surface_name(entry, cx), &theme))
					.child(panel_body(entry, "right-panel", &theme))
					.into_any_element()
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.flex_none()
			.gap(theme.space_16)
			.w(theme.member_list_width)
			.h_full()
			.p(theme.space_16)
			.overflow_hidden()
			.bg(theme.background_secondary)
			.children(sections)
	}
}

#[derive(IntoElement)]
pub struct DebugPanel;

impl RenderOnce for DebugPanel {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let bridge = cx.global::<PluginBridge>();
		let ui = bridge.ui().read(cx);
		if !ui.debug_visible() {
			return div().into_any_element();
		}
		let theme = cx.global::<Theme>().clone();
		let cell = |text: String, color: gpui::Hsla| {
			div()
				.flex_none()
				.w(rem(DEBUG_NUMBER_WIDTH))
				.text_right()
				.text_color(color)
				.child(text)
		};
		let header = div()
			.flex()
			.items_center()
			.h(rem(DEBUG_ROW_HEIGHT))
			.gap(theme.space_8)
			.text_size(theme.font_tiny)
			.font_weight(FontWeight::BOLD)
			.text_color(theme.header_secondary)
			.child(div().flex_1().child(cx.t("debug-plugin").to_uppercase()))
			.child(cell(
				cx.t("debug-calls").to_uppercase().to_string(),
				theme.header_secondary,
			))
			.child(cell(
				cx.t("debug-last").to_uppercase().to_string(),
				theme.header_secondary,
			))
			.child(cell(
				cx.t("debug-denied").to_uppercase().to_string(),
				theme.header_secondary,
			))
			.child(cell(
				cx.t("debug-crashes").to_uppercase().to_string(),
				theme.header_secondary,
			));
		let rows: Vec<AnyElement> = ui
			.debug()
			.iter()
			.map(|entry| {
				let name = bridge.name_of(&entry.id, cx);
				let flag = |count: u64, color: gpui::Hsla| {
					if count > 0 { color } else { theme.text_muted }
				};
				div()
					.flex()
					.items_center()
					.h(rem(DEBUG_ROW_HEIGHT))
					.gap(theme.space_8)
					.text_size(theme.font_xs)
					.text_color(theme.text_normal)
					.child(div().flex_1().min_w_0().truncate().child(name))
					.child(cell(entry.calls.to_string(), theme.text_normal))
					.child(cell(format!("{:.2}", entry.last_ms), theme.text_normal))
					.child(cell(entry.denied.to_string(), flag(entry.denied, theme.warning)))
					.child(cell(entry.crashes.to_string(), flag(entry.crashes, theme.danger)))
					.into_any_element()
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.flex_none()
			.max_h(rem(DEBUG_PANEL_MAX_HEIGHT))
			.overflow_hidden()
			.px(theme.space_16)
			.py(theme.space_8)
			.bg(theme.background_secondary)
			.border_t_1()
			.border_color(theme.divider)
			.child(header)
			.children(rows)
			.into_any_element()
	}
}
