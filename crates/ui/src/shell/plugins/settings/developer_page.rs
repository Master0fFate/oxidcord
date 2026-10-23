use std::rc::Rc;

use gpui::{
	Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window, div,
};
use gpui_component::switch::Switch;

use super::widgets::{card, heading, note};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::shell::SettingsBackend;
use crate::shell::plugins::bridge::PluginBridge;
use crate::theme::Theme;

pub struct DeveloperPage {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	debug: bool,
	_subscription: Subscription,
}

impl DeveloperPage {
	pub fn new(data: Entity<Data>, backend: Rc<SettingsBackend>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.plugins
			{
				cx.notify();
			}
		});
		let debug = (backend.read)().developer.show_plugin_debug;
		Self {
			data,
			backend,
			debug,
			_subscription: subscription,
		}
	}

	fn set_debug(&mut self, visible: bool, cx: &mut Context<Self>) {
		self.debug = visible;
		(self.backend.write)(&move |settings| settings.developer.show_plugin_debug = visible);
		let ui = cx.global::<PluginBridge>().ui().clone();
		ui.update(cx, |ui, cx| ui.set_debug_visible(visible, cx));
		cx.notify();
	}
}

impl Render for DeveloperPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let render_time = self.data.read(cx).session().plugin_render_ms_p95().map_or_else(
			|| cx.t("developer-render-none"),
			|ms| {
				cx.t_args(
					"developer-render-time",
					&[("ms", ArgValue::from(format!("{ms:.2}")))],
				)
			},
		);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				card(&theme)
					.child(
						Switch::new("developer-debug")
							.checked(self.debug)
							.label(cx.t("developer-debug"))
							.on_change(
								cx.listener(|this, checked: &bool, _, cx| this.set_debug(*checked, cx)),
							),
					)
					.child(note(cx.t("developer-debug-note"), &theme)),
			)
			.child(
				card(&theme)
					.child(heading(cx.t("developer-render"), &theme))
					.child(note(render_time, &theme)),
			)
	}
}
