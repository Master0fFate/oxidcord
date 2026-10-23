mod qr;
mod web;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use ::login::{QrConfig, QrSession, WebLoginSession};
use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _,
	Render, SharedString, Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Disableable as _, Sizable as _};
use session::login::{LoginError, validate_token};
use tokio::runtime::Handle;
use url::Url;

use self::qr::QrState;
use self::web::WebState;
use super::SettingsBackend;
use super::title_bar::TitleBar;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, with_alpha};

pub enum LoginEvent {
	Authenticated(String),
}

pub struct LoginOptions {
	pub rest_base: Url,
	pub qr_config: QrConfig,
	pub web_helper: Option<PathBuf>,
}

enum Phase {
	Idle,
	Checking,
	Failed(&'static str),
}

pub struct LoginScreen {
	input: Entity<InputState>,
	options: LoginOptions,
	backend: Rc<SettingsBackend>,
	runtime: Handle,
	acknowledged: bool,
	phase: Phase,
	qr_state: QrState,
	qr_session: Option<QrSession>,
	qr_task: Option<Task<()>>,
	web_state: WebState,
	web_session: Option<Arc<WebLoginSession>>,
	web_run: u64,
	_subscription: Subscription,
}

impl EventEmitter<LoginEvent> for LoginScreen {}

impl LoginScreen {
	pub fn new(
		options: LoginOptions,
		backend: Rc<SettingsBackend>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let placeholder = cx.t("login-token-placeholder");
		let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).masked(true));
		let subscription = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
			if let InputEvent::PressEnter { .. } = event {
				this.submit(cx);
			}
		});
		let acknowledged = (backend.read)().first_run_acknowledged;
		let mut screen = Self {
			input,
			options,
			backend,
			runtime: Handle::current(),
			acknowledged,
			phase: Phase::Idle,
			qr_state: QrState::Idle,
			qr_session: None,
			qr_task: None,
			web_state: WebState::Idle,
			web_session: None,
			web_run: 0,
			_subscription: subscription,
		};
		if acknowledged {
			screen.start_qr(cx);
		}
		screen
	}

	fn acknowledge(&mut self, cx: &mut Context<Self>) {
		self.acknowledged = true;
		(self.backend.write)(&|settings| settings.first_run_acknowledged = true);
		self.start_qr(cx);
		cx.notify();
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if !self.acknowledged || matches!(self.phase, Phase::Checking) {
			return;
		}
		let typed = self.input.read(cx).value().trim().to_owned();
		let token = if typed.is_empty() {
			platform::clipboard::read_text()
				.ok()
				.flatten()
				.map(|text| text.trim().to_owned())
				.unwrap_or_default()
		} else {
			typed
		};
		if token.is_empty() {
			self.phase = Phase::Failed("login-empty");
			cx.notify();
			return;
		}
		self.validate(token, cx);
	}

	fn validate(&mut self, token: String, cx: &mut Context<Self>) {
		self.phase = Phase::Checking;
		let (sender, receiver) = tokio::sync::oneshot::channel();
		let rest_base = self.options.rest_base.clone();
		let checked = token.clone();
		self.runtime.spawn(async move {
			let outcome = validate_token(&rest_base, &checked).await;
			let _ = sender.send(outcome);
		});
		cx.spawn(async move |this, cx| {
			let failure = match receiver.await {
				Ok(Ok(_)) => None,
				Ok(Err(LoginError::Rejected)) => Some("login-rejected"),
				Ok(Err(LoginError::Empty)) => Some("login-empty"),
				Ok(Err(LoginError::Rest(_))) | Err(_) => Some("login-network"),
			};
			this.update(cx, |this, cx| match failure {
				None => cx.emit(LoginEvent::Authenticated(token)),
				Some(key) => {
					this.phase = Phase::Failed(key);
					cx.notify();
				}
			})
			.ok();
		})
		.detach();
		cx.notify();
	}

	fn brand(&self, theme: &Theme, cx: &Context<Self>) -> AnyElement {
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(
				div()
					.flex()
					.items_center()
					.justify_center()
					.size(theme.login_logo)
					.rounded_full()
					.bg(theme.brand)
					.child(
						Icon::new(IconKind::Chat)
							.size(theme.login_logo_icon)
							.color(theme.text_on_brand),
					),
			)
			.child(
				div()
					.text_size(theme.font_xl)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(cx.t("login-title")),
			)
			.child(
				div()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t("login-subtitle")),
			)
			.into_any_element()
	}

	fn token_column(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let checking = matches!(self.phase, Phase::Checking);
		let status: Option<SharedString> = match self.phase {
			Phase::Idle => None,
			Phase::Checking => Some(cx.t("login-checking")),
			Phase::Failed(key) => Some(cx.t(key)),
		};
		let failed = matches!(self.phase, Phase::Failed(_));
		let locked = !self.acknowledged;
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_w_0()
			.gap(theme.space_16)
			.child(self.brand(theme, cx))
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(
						div()
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(cx.t("login-token-label").to_uppercase()),
					)
					.child(Input::new(&self.input).large().mask_toggle())
					.children(status.map(|status| {
						div()
							.text_size(theme.font_sm)
							.when(failed, |line| line.text_color(theme.danger))
							.when(!failed, |line| line.text_color(theme.text_muted))
							.child(status)
					})),
			)
			.child(
				Button::new("login-paste")
					.primary()
					.large()
					.w_full()
					.loading(checking)
					.disabled(locked)
					.label(cx.t("login-paste"))
					.on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
			)
			.child(self.web_controls(locked, theme, cx))
			.into_any_element()
	}

	fn notice(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let acknowledged = self.acknowledged;
		div()
			.flex()
			.flex_col()
			.gap(theme.space_12)
			.w(theme.login_card_width)
			.p(theme.space_16)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(with_alpha(theme.warning, 0.5))
			.bg(with_alpha(theme.warning, 0.1))
			.child(
				div()
					.flex()
					.items_start()
					.gap(theme.space_12)
					.child(
						Icon::new(IconKind::Warning)
							.size(theme.icon_lg)
							.color(theme.warning),
					)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.text_size(theme.font_sm)
							.text_color(theme.text_normal)
							.child(cx.t("login-notice")),
					),
			)
			.child(
				Checkbox::new("login-acknowledge")
					.checked(acknowledged)
					.disabled(acknowledged)
					.label(cx.t("login-understand"))
					.on_click(cx.listener(|this, checked: &bool, _, cx| {
						if *checked {
							this.acknowledge(cx);
						}
					})),
			)
			.into_any_element()
	}
}

impl Render for LoginScreen {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let token = self.token_column(&theme, cx);
		let qr = self.qr_column(&theme, cx);
		let notice = self.notice(&theme, cx);
		div()
			.flex()
			.flex_col()
			.size_full()
			.bg(theme.background_tertiary)
			.text_color(theme.text_normal)
			.text_size(theme.font_md)
			.font_family(theme.ui_font.clone())
			.child(TitleBar)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.items_center()
					.justify_center()
					.gap(theme.space_16)
					.child(
						div()
							.flex()
							.gap(theme.space_32)
							.w(theme.login_card_width)
							.p(theme.space_32)
							.rounded(theme.radius_md)
							.bg(theme.background_secondary)
							.child(token)
							.child(qr),
					)
					.child(notice),
			)
	}
}
