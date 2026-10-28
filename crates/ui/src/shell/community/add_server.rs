use std::rc::Rc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, FontWeight, IntoElement,
	ObjectFit, ParentElement as _, Render, SharedString, Styled as _, StyledImage as _, Subscription, Window,
	div, img,
};
use gpui_component::button::Button;
use gpui_component::input::{Input, InputEvent, InputState};
use session::Command;

use super::files::{Fit, IMAGE_TYPES, Picked, pick_image};
use super::invites::stat;
use super::modal::{self, Close};
use crate::components::{Avatar, PickHandler, dialog_footer, field, hint, segmented};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::theme::Theme;

const ICON_SIDE: u32 = 512;
const NAME_LIMIT: usize = 100;
const CODE_MIN: usize = 2;
const CODE_MAX: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
	Join,
	Create,
}

pub struct AddServer {
	data: Entity<Data>,
	path: Path,
	code: Entity<InputState>,
	name: Entity<InputState>,
	icon: Option<Picked>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<Close> for AddServer {}

pub fn open(window: &mut Window, cx: &mut App) {
	let data = super::data_handle(cx);
	let view = cx.new(|cx| AddServer::new(data, window, cx));
	let focus = view.read(cx).code.read(cx).focus_handle(cx);
	let title = cx.t("add-server-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

fn invite_code(input: &str) -> Option<String> {
	let cleaned = input.trim().split(['?', '#']).next()?.trim_end_matches('/');
	let tail = cleaned.rsplit('/').next()?;
	let valid = (CODE_MIN..=CODE_MAX).contains(&tail.len())
		&& tail
			.chars()
			.all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
	valid.then(|| tail.to_owned())
}

impl AddServer {
	fn new(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let code_placeholder = cx.t("add-server-code-placeholder");
		let code = cx.new(|cx| InputState::new(window, cx).placeholder(code_placeholder));
		let default_name = {
			let state = data.read(cx).state();
			state
				.current_user_id()
				.map(|me| state.display_name(None, me))
				.unwrap_or_default()
		};
		let default_name = cx.t_args(
			"add-server-default-name",
			&[("name", crate::i18n::ArgValue::from(default_name))],
		);
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(default_name.to_string(), window, cx);
			state
		});
		let subscriptions = vec![
			cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
				if matches!(event, DataEvent::Changed(_)) {
					cx.notify();
				}
			}),
			cx.subscribe_in(
				&code,
				window,
				|this, _, event: &InputEvent, window, cx| match event {
					InputEvent::PressEnter { .. } => this.submit(window, cx),
					InputEvent::Change => cx.notify(),
					_ => {}
				},
			),
			cx.subscribe_in(
				&name,
				window,
				|this, _, event: &InputEvent, window, cx| match event {
					InputEvent::PressEnter { .. } => this.submit(window, cx),
					InputEvent::Change => cx.notify(),
					_ => {}
				},
			),
		];
		Self {
			data,
			path: Path::Join,
			code,
			name,
			icon: None,
			_subscriptions: subscriptions,
		}
	}

	fn typed_code(&self, cx: &App) -> Option<String> {
		invite_code(self.code.read(cx).value().as_ref())
	}

	fn typed_name(&self, cx: &App) -> String {
		self.name
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(NAME_LIMIT)
			.collect()
	}

	fn joined(&self, code: &str, cx: &App) -> Option<bool> {
		let data = self.data.read(cx);
		let state = data.state();
		let preview = state.invite_preview(code)?;
		Some(preview.guild_id.is_some_and(|id| state.guild(id).is_some()))
	}

	fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		match self.path {
			Path::Join => self.submit_join(cx),
			Path::Create => self.submit_create(cx),
		}
	}

	fn submit_join(&mut self, cx: &mut Context<Self>) {
		let Some(code) = self.typed_code(cx) else {
			return;
		};
		match self.joined(&code, cx) {
			None => self.data.read(cx).send(Command::ResolveInvite { code }),
			Some(true) => {}
			Some(false) => {
				self.data.read(cx).send(Command::AcceptInvite { code });
				cx.emit(Close);
			}
		}
	}

	fn submit_create(&mut self, cx: &mut Context<Self>) {
		let name = self.typed_name(cx);
		if name.is_empty() {
			return;
		}
		let icon = self
			.icon
			.take()
			.map(|picked| format!("data:image/png;base64,{}", STANDARD.encode(picked.bytes)));
		self.data.read(cx).send(Command::CreateGuild { name, icon });
		cx.emit(Close);
	}

	fn preview_card(&self, code: &str, theme: &Theme, cx: &App) -> Option<AnyElement> {
		let data = self.data.read(cx);
		let state = data.state();
		let preview = state.invite_preview(code)?;
		let avatar = Avatar::new(preview.guild_name.clone()).size(theme.invite_card_icon);
		let avatar = match preview.guild_id.zip(preview.guild_icon.as_deref()) {
			Some((id, hash)) => avatar.url(media::cdn::guild_icon_url(id.get(), hash, 128)),
			None => avatar,
		};
		Some(
			div()
				.flex()
				.items_center()
				.gap(theme.space_16)
				.p(theme.space_16)
				.rounded(theme.radius_md)
				.border_1()
				.border_color(theme.divider)
				.bg(theme.background_secondary)
				.child(avatar)
				.child(
					div()
						.flex()
						.flex_col()
						.min_w_0()
						.gap(theme.space_4)
						.child(
							div()
								.truncate()
								.text_size(theme.font_md)
								.font_weight(FontWeight::BOLD)
								.text_color(theme.header_primary)
								.child(preview.guild_name.clone()),
						)
						.child(
							div()
								.flex()
								.items_center()
								.gap(theme.space_12)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(stat(
									theme.status_online,
									preview.online_count,
									"invite-card-online",
									theme,
									cx,
								))
								.child(stat(
									theme.status_offline,
									preview.member_count,
									"invite-card-members",
									theme,
									cx,
								)),
						),
				)
				.into_any_element(),
		)
	}

	fn join_body(&self, theme: &Theme, cx: &mut Context<Self>) -> (AnyElement, SharedString, bool) {
		let code = self.typed_code(cx);
		let joined = code.as_deref().and_then(|code| self.joined(code, cx));
		let preview = code
			.as_deref()
			.and_then(|code| self.preview_card(code, theme, cx));
		let body = div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(field(cx.t("add-server-code"), theme, Input::new(&self.code)))
			.child(hint(cx.t("add-server-code-hint"), theme))
			.children(preview)
			.into_any_element();
		let label = cx.t(match joined {
			None => "add-server-find",
			Some(false) => "add-server-join-confirm",
			Some(true) => "invite-card-joined",
		});
		(body, label, code.is_some() && joined != Some(true))
	}

	fn create_body(&self, theme: &Theme, cx: &mut Context<Self>) -> (AnyElement, SharedString, bool) {
		let name = self.typed_name(cx);
		let tile = div()
			.flex_none()
			.size(theme.portrait_lg * 2.)
			.rounded_full()
			.overflow_hidden()
			.child(match &self.icon {
				Some(picked) => img(picked.preview.clone())
					.size_full()
					.object_fit(ObjectFit::Cover)
					.into_any_element(),
				None => Avatar::new(SharedString::from(name.clone()))
					.size(theme.portrait_lg * 2.)
					.into_any_element(),
			});
		let body = div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div().flex().items_center().gap(theme.space_16).child(tile).child(
					div()
						.flex()
						.flex_col()
						.items_start()
						.gap(theme.space_8)
						.child(
							Button::new("add-server-icon")
								.outline()
								.icon(gpui_component::Icon::default().path(IconKind::Upload.path()))
								.label(cx.t("server-overview-upload-icon"))
								.on_click(cx.listener(|_, _, _, cx| {
									pick_image(
										cx,
										Fit::Square(ICON_SIDE),
										IMAGE_TYPES,
										|this: &mut Self, picked, cx| {
											this.icon = Some(picked);
											cx.notify();
										},
									)
								})),
						)
						.child(hint(cx.t("server-overview-icon-hint"), theme)),
				),
			)
			.child(field(cx.t("server-overview-name"), theme, Input::new(&self.name)))
			.into_any_element();
		(body, cx.t("add-server-create-confirm"), !name.is_empty())
	}
}

impl Render for AddServer {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let labels = [cx.t("add-server-join"), cx.t("add-server-create")];
		let selected = usize::from(self.path == Path::Create);
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.path = if index == 0 { Path::Join } else { Path::Create };
				cx.notify();
			})
			.ok();
		});
		let (body, confirm, enabled) = match self.path {
			Path::Join => self.join_body(&theme, cx),
			Path::Create => self.create_body(&theme, cx),
		};
		let weak_cancel = cx.weak_entity();
		let weak_confirm = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(segmented("add-server-path", &labels, selected, &theme, pick))
			.child(body)
			.child(dialog_footer(
				cx.t("action-cancel"),
				confirm,
				false,
				enabled,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |window, cx| {
					weak_confirm.update(cx, |this, cx| this.submit(window, cx)).ok();
				},
			))
	}
}
