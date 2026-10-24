mod developer_page;
mod permission;
mod plugin_form;
mod plugins_page;
mod themes_page;
mod widgets;

use std::sync::Arc;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
	prelude::FluentBuilder as _,
};

pub use self::developer_page::DeveloperPage;
pub use self::plugins_page::PluginsPage;
pub use self::themes_page::{ThemeApplied, ThemesPage};
use self::widgets::{card, icon_tile, note};
use super::bridge::PluginBridge;
use super::slot_renderer::{Scope, SlotHost, render_column};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const NAV_ICON: f32 = 18.;
const NAV_HEIGHT: f32 = 32.;
const NAV_PADDING: f32 = 10.;

pub struct SlotPage {
	selected: Option<Arc<str>>,
	_subscription: Subscription,
}

impl SlotPage {
	fn new(data: &Entity<Data>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(data, |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.plugin_slots
			{
				cx.notify();
			}
		});
		Self {
			selected: None,
			_subscription: subscription,
		}
	}

	pub fn select(&mut self, plugin: Arc<str>, cx: &mut Context<Self>) {
		self.selected = Some(plugin);
		cx.notify();
	}
}

impl Render for SlotPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let host = SlotHost::new("settings-page");
		let entries = host.entries(cx);
		let entry = entries
			.iter()
			.find(|entry| Some(&entry.plugin_id) == self.selected.as_ref())
			.or_else(|| entries.first());
		let Some(entry) = entry else {
			return note(cx.t("plugin-slot-empty"), &theme);
		};
		let name = cx.global::<PluginBridge>().name_of(&entry.plugin_id, cx);
		let scope: Scope = host.scope(entry);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_12)
					.child(icon_tile(IconKind::Plugins, &theme))
					.child(
						div()
							.text_size(theme.font_lg)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(name),
					),
			)
			.child(card(&theme).child(render_column(&entry.tree, &scope, &theme)))
			.into_any_element()
	}
}

#[derive(Clone)]
pub struct PluginPages {
	pub plugins: Entity<PluginsPage>,
	pub themes: Entity<ThemesPage>,
	pub developer: Entity<DeveloperPage>,
	pub slot: Entity<SlotPage>,
}

impl PluginPages {
	pub fn new<T: 'static>(window: &mut Window, cx: &mut Context<T>) -> Self {
		let (data, backend) = {
			let bridge = cx.global::<PluginBridge>();
			(bridge.data().clone(), bridge.backend().clone())
		};
		Self {
			plugins: cx.new({
				let (data, backend) = (data.clone(), backend.clone());
				|cx| PluginsPage::new(data, backend, window, cx)
			}),
			themes: cx.new({
				let (data, backend) = (data.clone(), backend.clone());
				|cx| ThemesPage::new(data, backend, window, cx)
			}),
			slot: cx.new(|cx| SlotPage::new(&data, cx)),
			developer: cx.new(|cx| DeveloperPage::new(data, backend, cx)),
		}
	}

	pub fn nav_items<T: 'static>(
		&self,
		active: bool,
		theme: &Theme,
		cx: &mut Context<T>,
		on_select: impl Fn(&mut T, &mut Context<T>) + Clone + 'static,
	) -> Vec<AnyElement> {
		let selected = self.slot.read(cx).selected.clone();
		let entries = SlotHost::new("settings-page").entries(cx);
		let first = entries.first().map(|entry| entry.plugin_id.clone());
		entries
			.iter()
			.enumerate()
			.map(|(index, entry)| {
				let plugin = entry.plugin_id.clone();
				let name = cx.global::<PluginBridge>().name_of(&plugin, cx);
				let current = selected.as_ref().or(first.as_ref()) == Some(&plugin);
				let highlighted = active && current;
				let (rest, hover) = if highlighted {
					(theme.interactive_active, theme.interactive_active)
				} else {
					(theme.interactive_normal, theme.interactive_hover)
				};
				let slot = self.slot.clone();
				let on_select = on_select.clone();
				let hover_background = theme.modifier_hover;
				div()
					.id(("settings-plugin-page", index))
					.flex()
					.items_center()
					.gap(theme.space_8)
					.h(rem(NAV_HEIGHT))
					.px(rem(NAV_PADDING))
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.text_size(theme.font_md)
					.font_weight(FontWeight::MEDIUM)
					.text_color(rest)
					.when(highlighted, |item| item.bg(theme.modifier_selected))
					.when(!highlighted, |item| {
						item.hover(move |style| style.bg(hover_background).text_color(hover))
					})
					.on_click(cx.listener(move |this, _, _, cx| {
						let plugin = plugin.clone();
						slot.update(cx, |slot, cx| slot.select(plugin, cx));
						on_select(this, cx);
					}))
					.child(Icon::new(IconKind::Plugins).size(rem(NAV_ICON)).color(rest))
					.child(div().min_w_0().truncate().child(name))
					.into_any_element()
			})
			.collect()
	}
}
