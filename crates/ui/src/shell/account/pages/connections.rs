use std::rc::Rc;

use gpui::{
	AnyElement, Context, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
	Subscription, Window, div,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::switch::Switch;
use protocol::Connection;
use serde_json::json;
use session::Command;
use state::CONNECTIONS_KEY;

use super::super::{parts, providers};
use super::my_account::open_web_app;
use super::widgets::{row, row_with_lead};
use crate::data::{Data, DataEvent, streamer_mode};
use crate::i18n::Translate as _;
use crate::theme::Theme;

const MASKED_NAME: &str = "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}";
const VISIBLE_TO_EVERYONE: i64 = 1;

pub struct ConnectionsPage {
	data: Entity<Data>,
	connections: Option<Vec<Connection>>,
	removing: Option<(String, String)>,
	_subscription: Subscription,
}

impl ConnectionsPage {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |this, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.loaded.contains(CONNECTIONS_KEY)
			{
				this.connections = this.load_connections(cx);
				this.removing = None;
				cx.notify();
			}
		});
		let mut page = Self {
			data,
			connections: None,
			removing: None,
			_subscription: subscription,
		};
		page.connections = page.load_connections(cx);
		page
	}

	fn load_connections(&self, cx: &Context<Self>) -> Option<Vec<Connection>> {
		self.data.read(cx).state().connections().map(<[_]>::to_vec)
	}

	pub fn refresh(&self, cx: &Context<Self>) {
		self.data.read(cx).send(Command::LoadConnections);
	}

	fn update(&self, key: &(String, String), changes: serde_json::Value, cx: &Context<Self>) {
		self.data.read(cx).send(Command::UpdateConnection {
			kind: key.0.clone(),
			id: key.1.clone(),
			changes,
		});
	}

	fn remove(&mut self, key: &(String, String), cx: &mut Context<Self>) {
		if self.removing.as_ref() == Some(key) {
			let (kind, id) = key.clone();
			self.data.read(cx).send(Command::RemoveConnection { kind, id });
			self.removing = None;
		} else {
			self.removing = Some(key.clone());
		}
		cx.notify();
	}

	fn connection_block(
		&self,
		index: usize,
		connection: &Connection,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let name: SharedString = if streamer_mode(cx).hides_personal() {
			MASKED_NAME.into()
		} else {
			connection.name.clone().into()
		};
		let confirming = self
			.removing
			.as_ref()
			.is_some_and(|(kind, id)| *kind == connection.kind && *id == connection.id);
		let key = Rc::new((connection.kind.clone(), connection.id.clone()));
		let target = key.clone();
		let remove = Button::new(("connection-remove", index))
			.small()
			.danger()
			.label(cx.t(if confirming {
				"connections-confirm"
			} else {
				"connections-remove"
			}))
			.on_click(cx.listener(move |this, _, _, cx| this.remove(&target, cx)));
		let visible = connection.visibility == VISIBLE_TO_EVERYONE;
		let target = key.clone();
		let visibility = Switch::new(("connection-visible", index))
			.checked(visible)
			.on_change(cx.listener(move |this, checked: &bool, _, cx| {
				let visibility = i64::from(*checked);
				this.update(&target, json!({ "visibility": visibility }), cx);
			}));
		let target = key;
		let activity = providers::shows_activity(&connection.kind).then(|| {
			row(
				cx.t("connections-status"),
				None,
				Switch::new(("connection-activity", index))
					.checked(connection.show_activity)
					.on_change(cx.listener(move |this, checked: &bool, _, cx| {
						this.update(&target, json!({ "show_activity": *checked }), cx);
					})),
				theme,
			)
		});
		div()
			.flex()
			.flex_col()
			.pb(theme.space_16)
			.child(row_with_lead(
				Some(parts::provider_icon(&connection.kind, theme)),
				SharedString::from(providers::label(&connection.kind).to_owned()),
				Some(name),
				remove,
				theme,
			))
			.child(row(cx.t("connections-profile"), None, visibility, theme))
			.children(activity)
			.into_any_element()
	}
}

impl Render for ConnectionsPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let blocks: Vec<AnyElement> = self
			.connections
			.iter()
			.flatten()
			.enumerate()
			.map(|(index, connection)| self.connection_block(index, connection, &theme, cx))
			.collect();
		let status = match &self.connections {
			None => Some("connections-loading"),
			Some(found) if found.is_empty() => Some("connections-empty"),
			Some(_) => None,
		};
		div()
			.flex()
			.flex_col()
			.children(status.map(|label| {
				div()
					.pb(theme.space_16)
					.text_size(theme.font_md)
					.text_color(theme.text_muted)
					.child(cx.t(label))
			}))
			.children(blocks)
			.child(row(
				cx.t("connections-add"),
				Some(cx.t("connections-add-note")),
				Button::new("connections-add")
					.small()
					.label(cx.t("account-open-web"))
					.on_click(|_, _, _| open_web_app()),
				&theme,
			))
	}
}
