use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Render,
	SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::button::Button;
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::Snowflake;
use session::Command;

use crate::components::{PickHandler, dialog_footer, field, hint, select};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::shell::community::data_handle;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const CANDIDATE_LIMIT: usize = 100;

struct TransferServer {
	data: Entity<Data>,
	guild: Snowflake,
	candidates: Vec<(Snowflake, SharedString)>,
	picked: Option<usize>,
	needs_code: bool,
	code_sent: bool,
	code: Entity<InputState>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for TransferServer {}

fn candidates(data: &Data, guild: Snowflake) -> Vec<(Snowflake, SharedString)> {
	let state = data.state();
	let me = state.current_user_id();
	let Some(found) = state.guild(guild) else {
		return Vec::new();
	};
	let mut people: Vec<(Snowflake, SharedString)> = found
		.members
		.keys()
		.copied()
		.filter(|id| Some(*id) != me)
		.filter(|id| !state.user(*id).is_some_and(|user| user.bot == Some(true)))
		.map(|id| (id, state.display_name(Some(guild), id).into()))
		.collect();
	people.sort_by_cached_key(|(_, name)| name.to_lowercase());
	people.truncate(CANDIDATE_LIMIT);
	people
}

pub fn open(window: &mut Window, cx: &mut App, guild: Snowflake, name: String) {
	let data = data_handle(cx);
	let title = cx.t_args("server-transfer-title", &[("name", ArgValue::from(name))]);
	let view = cx.new(|cx| {
		let code = cx.new(|cx| InputState::new(window, cx));
		let subscription = cx.subscribe_in(&code, window, |_, _, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		});
		let needs_code = data
			.read(cx)
			.state()
			.current_user()
			.is_some_and(|user| user.email.is_some() && user.mfa_enabled != Some(true));
		TransferServer {
			candidates: candidates(data.read(cx), guild),
			data,
			guild,
			picked: None,
			needs_code,
			code_sent: false,
			code,
			_subscription: subscription,
		}
	});
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, None);
}

impl TransferServer {
	fn code_text(&self, cx: &App) -> Option<String> {
		let text = self.code.read(cx).value().trim().to_owned();
		(!text.is_empty()).then_some(text)
	}

	fn ready(&self, cx: &App) -> bool {
		self.picked.is_some() && (!self.needs_code || self.code_text(cx).is_some())
	}

	fn transfer(&self, cx: &mut Context<Self>) {
		let Some((owner_id, _)) = self.picked.and_then(|index| self.candidates.get(index)) else {
			return;
		};
		self.data.read(cx).send(Command::TransferGuild {
			guild_id: self.guild,
			owner_id: *owner_id,
			code: self.code_text(cx),
		});
		cx.emit(Close);
	}

	fn send_code(&mut self, cx: &mut Context<Self>) {
		self.data
			.read(cx)
			.send(Command::RequestOwnershipCode { guild_id: self.guild });
		self.code_sent = true;
		cx.notify();
	}

	fn code_row(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let send = Button::new("transfer-send-code")
			.outline()
			.label(cx.t("server-transfer-send"))
			.on_click(cx.listener(|this, _, _, cx| this.send_code(cx)));
		let note = hint(
			cx.t(if self.code_sent {
				"server-transfer-sent"
			} else {
				"server-transfer-code-hint"
			}),
			theme,
		);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(field(
				cx.t("server-transfer-code"),
				theme,
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(div().flex_1().min_w_0().child(Input::new(&self.code)))
					.child(send),
			))
			.child(note)
			.into_any_element()
	}
}

impl Render for TransferServer {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let labels: Vec<SharedString> = self.candidates.iter().map(|(_, name)| name.clone()).collect();
		let current = self
			.picked
			.and_then(|index| labels.get(index).cloned())
			.unwrap_or_else(|| cx.t("server-transfer-pick"));
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.picked = Some(index);
				cx.notify();
			})
			.ok();
		});
		let weak_cancel = cx.weak_entity();
		let weak_confirm = cx.weak_entity();
		let ready = self.ready(cx);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(hint(cx.t("server-transfer-body"), &theme))
			.child(field(
				cx.t("server-transfer-member"),
				&theme,
				select(
					"server-transfer-member",
					current,
					&labels,
					self.picked.unwrap_or(usize::MAX),
					pick,
				),
			))
			.children(self.needs_code.then(|| self.code_row(&theme, cx)))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("server-transfer-confirm"),
				true,
				ready,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_confirm.update(cx, |this, cx| this.transfer(cx)).ok();
				},
			))
	}
}
