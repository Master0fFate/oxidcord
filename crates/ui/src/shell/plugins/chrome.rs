use gpui::{
	AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
	Window, div, prelude::FluentBuilder as _,
};

use super::bridge::PluginBridge;
use super::slot_renderer::{COMPOSER, STATUS, Scope, SlotHost, TITLE, render_column};
use crate::components::{IconButton, SectionHeader};
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::theme::{Theme, rem};

const STATUS_BAR_HEIGHT: f32 = 24.;
const SIDEBAR_PANEL_MAX_HEIGHT: f32 = 260.;
const DEBUG_PANEL_MAX_HEIGHT: f32 = 220.;
const DEBUG_ROW_HEIGHT: f32 = 24.;
const DEBUG_NUMBER_WIDTH: f32 = 84.;

fn elements(items: Vec<(std::sync::Arc<str>, AnyElement)>) -> Vec<AnyElement> {
	items.into_iter().map(|(_, element)| element).collect()
}

pub fn composer_toolbar(cx: &App) -> Vec<AnyElement> {
	elements(SlotHost::new("composer-toolbar").compact(COMPOSER).render(cx))
}

pub fn title_items(cx: &App) -> Vec<AnyElement> {
	if !cx.has_global::<PluginBridge>() {
		return Vec::new();
	}
	elements(SlotHost::new("title-bar").compact(TITLE).render(cx))
}

pub fn status_bar(cx: &App) -> Option<AnyElement> {
	let items = elements(SlotHost::new("status-bar").compact(STATUS).render(cx));
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
	if bridge.entries("right-panel", cx).is_empty() {
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
	bridge.ui().read(cx).right_open() && !bridge.entries("right-panel", cx).is_empty()
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
		let host = SlotHost::new("sidebar-panel");
		let entries = host.entries(cx);
		if entries.is_empty() {
			return div().into_any_element();
		}
		let theme = cx.global::<Theme>().clone();
		let sections: Vec<AnyElement> = entries
			.iter()
			.map(|entry| {
				let key = SharedString::from(format!("plugin-sidebar:{}", entry.plugin_id));
				let collapsed = window.use_keyed_state(key.clone(), cx, |_, _| false);
				let is_collapsed = *collapsed.read(cx);
				let name = cx.global::<PluginBridge>().name_of(&entry.plugin_id, cx);
				let scope = host.scope(entry);
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
						(!is_collapsed).then(|| render_column(&entry.tree, &scope, &theme)),
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
		let host = SlotHost::new("right-panel");
		let sections: Vec<AnyElement> = host
			.entries(cx)
			.iter()
			.map(|entry| {
				let name = cx.global::<PluginBridge>().name_of(&entry.plugin_id, cx);
				let scope: Scope = host.scope(entry);
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(section_label(name, &theme))
					.child(render_column(&entry.tree, &scope, &theme))
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
