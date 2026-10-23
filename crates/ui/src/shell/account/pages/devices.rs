use gpui::{
	AnyElement, Context, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
	Subscription, Window, div,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use session::Command;
use state::AuthSession;

use super::widgets::row;
use crate::data::{Data, DataEvent};
use crate::format::message_time;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

pub struct DevicesPage {
	data: Entity<Data>,
	_subscription: Subscription,
}

fn capitalized(text: &str) -> String {
	let mut characters = text.chars();
	match characters.next() {
		Some(first) => first.to_uppercase().chain(characters).collect(),
		None => String::new(),
	}
}

impl DevicesPage {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& !changes.loaded.is_empty()
			{
				cx.notify();
			}
		});
		Self {
			data,
			_subscription: subscription,
		}
	}

	pub fn refresh(&self, cx: &Context<Self>) {
		self.data.read(cx).send(Command::LoadSessions);
	}

	fn log_out(&self, hashes: Vec<String>, cx: &Context<Self>) {
		self.data.read(cx).send(Command::LogoutSessions(hashes));
	}

	fn device_row(
		&self,
		index: usize,
		session: &AuthSession,
		current: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let title = if session.os.is_empty() {
			cx.t("devices-unknown")
		} else {
			capitalized(&session.os).into()
		};
		let mut details: Vec<SharedString> = Vec::new();
		if !session.platform.is_empty() {
			details.push(capitalized(&session.platform).into());
		}
		if !session.location.is_empty() {
			details.push(session.location.clone().into());
		}
		if current {
			details.push(cx.t("devices-this"));
		} else if let Some(time) = session.last_used {
			details.push(cx.t_args(
				"devices-last-used",
				&[("time", ArgValue::from(message_time(time, cx).to_string()))],
			));
		}
		let note: SharedString = details
			.iter()
			.map(SharedString::as_ref)
			.collect::<Vec<_>>()
			.join(" \u{b7} ")
			.into();
		let hash = session.id_hash.clone();
		let action = (!current).then(|| {
			Button::new(("devices-log-out", index))
				.small()
				.label(cx.t("devices-log-out"))
				.on_click(cx.listener(move |this, _, _, cx| this.log_out(vec![hash.clone()], cx)))
		});
		row(
			title,
			Some(note),
			div()
				.flex()
				.items_center()
				.gap(theme.space_12)
				.child(
					Icon::new(IconKind::Devices)
						.size(theme.icon_lg)
						.color(theme.interactive_muted),
				)
				.children(action),
			theme,
		)
	}
}

impl Render for DevicesPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let sessions = self.data.read(cx).state().auth_sessions().map(|found| {
			let mut sessions = found.to_vec();
			sessions.sort_by_key(|session| std::cmp::Reverse(session.last_used));
			sessions
		});
		let Some(sessions) = sessions else {
			return div()
				.text_size(theme.font_md)
				.text_color(theme.text_muted)
				.child(cx.t("devices-loading"))
				.into_any_element();
		};
		if sessions.is_empty() {
			return div()
				.text_size(theme.font_md)
				.text_color(theme.text_muted)
				.child(cx.t("devices-empty"))
				.into_any_element();
		}
		let others: Vec<String> = sessions
			.iter()
			.skip(1)
			.map(|session| session.id_hash.clone())
			.collect();
		let mut rows = Vec::with_capacity(sessions.len());
		for (index, session) in sessions.iter().enumerate() {
			rows.push(self.device_row(index, session, index == 0, &theme, cx));
		}
		div()
			.flex()
			.flex_col()
			.children(rows)
			.children((!others.is_empty()).then(|| {
				div().pt(theme.space_16).child(
					Button::new("devices-log-out-others")
						.danger()
						.label(cx.t("devices-log-out-others"))
						.on_click(cx.listener(move |this, _, _, cx| this.log_out(others.clone(), cx))),
				)
			}))
			.into_any_element()
	}
}
