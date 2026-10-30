use gpui::{
	AnyElement, App, ClickEvent, Context, FontWeight, Hsla, InteractiveElement as _, IntoElement,
	ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::Icon as ComponentIcon;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::DropdownMenu as _;
use protocol::Snowflake;
use session::Command;
use state::RelationshipRow;

use super::{FriendsEvent, FriendsPage};
use crate::components::{Avatar, ContextMenuBuilder, tooltip};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::model::Status;
use crate::theme::Theme;

const AVATAR_PIXELS: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
	Friend,
	Incoming,
	Outgoing,
	Blocked,
}

pub(super) fn status_key(status: Status) -> &'static str {
	match status {
		Status::Online => "status-online",
		Status::Idle => "status-idle",
		Status::Dnd => "status-dnd",
		Status::Offline => "status-offline",
	}
}

type Handler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

fn circle_button(
	id: impl Into<gpui::ElementId>,
	icon: IconKind,
	hint: SharedString,
	hover: Hsla,
	theme: &Theme,
	handler: Handler,
) -> AnyElement {
	let group: SharedString = format!("friend-circle-{}", id.into()).into();
	div()
		.id(group.clone())
		.group(group.clone())
		.flex()
		.items_center()
		.justify_center()
		.size(theme.avatar_sm + theme.space_4)
		.rounded(theme.round(theme.avatar_sm + theme.space_4))
		.bg(theme.background_secondary)
		.cursor_pointer()
		.tooltip(tooltip(hint))
		.on_click(move |event, window, cx| {
			cx.stop_propagation();
			handler(event, window, cx);
		})
		.child(
			Icon::new(icon)
				.size(theme.icon_md)
				.color(theme.interactive_normal)
				.group_hover(group, hover),
		)
		.into_any_element()
}

impl FriendsPage {
	fn subtitle(
		&self,
		item: &RelationshipRow,
		kind: Kind,
		status: Status,
		cx: &Context<Self>,
	) -> SharedString {
		match kind {
			Kind::Incoming => cx.t("friends-incoming-request"),
			Kind::Outgoing => cx.t("friends-outgoing-request"),
			Kind::Blocked => cx.t("friends-blocked"),
			Kind::Friend => item
				.activity
				.clone()
				.map_or_else(|| cx.t(status_key(status)), Into::into),
		}
	}

	fn actions(
		&self,
		item: &RelationshipRow,
		kind: Kind,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let user = item.user_id;
		let data = self.data.clone();
		let send = move |command: fn(Snowflake) -> Command| {
			let data = data.clone();
			Box::new(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
				data.read(cx).send(command(user));
			}) as Handler
		};
		let key = |name: &str| format!("{name}-{user}");
		match kind {
			Kind::Friend => {
				let weak = cx.weak_entity();
				let message: Handler = Box::new(move |_, _, cx| {
					weak.update(cx, |this, cx| this.message(user, cx)).ok();
				});
				vec![
					circle_button(
						key("friend-message"),
						IconKind::Chat,
						cx.t("friends-message"),
						theme.interactive_hover,
						theme,
						message,
					),
					self.more_button(user, theme, cx),
				]
			}
			Kind::Incoming => vec![
				circle_button(
					key("friend-accept"),
					IconKind::Check,
					cx.t("friends-accept"),
					theme.success,
					theme,
					send(|user_id| Command::AcceptFriend { user_id }),
				),
				circle_button(
					key("friend-ignore"),
					IconKind::Close,
					cx.t("friends-ignore"),
					theme.danger,
					theme,
					send(|user_id| Command::RemoveRelationship { user_id }),
				),
			],
			Kind::Outgoing => vec![circle_button(
				key("friend-cancel"),
				IconKind::Close,
				cx.t("friends-cancel"),
				theme.danger,
				theme,
				send(|user_id| Command::RemoveRelationship { user_id }),
			)],
			Kind::Blocked => vec![circle_button(
				key("friend-unblock"),
				IconKind::Close,
				cx.t("friends-unblock"),
				theme.danger,
				theme,
				send(|user_id| Command::RemoveRelationship { user_id }),
			)],
		}
	}

	fn more_button(&self, user: Snowflake, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let remove = self.data.clone();
		let block = self.data.clone();
		let menu = ContextMenuBuilder::new()
			.danger_item(cx.t("friends-remove"), Some(IconKind::Trash), move |_, cx| {
				remove
					.read(cx)
					.send(Command::RemoveRelationship { user_id: user });
			})
			.danger_item(cx.t("friends-block"), Some(IconKind::Shield), move |_, cx| {
				block.read(cx).send(Command::BlockUser { user_id: user });
			})
			.separator()
			.item(cx.t("menu-copy-user-id"), Some(IconKind::Copy), move |_, _| {
				if let Err(error) = platform::clipboard::write_text(&user.to_string()) {
					tracing::warn!(%error, "could not copy the user id");
				}
			})
			.build();
		div()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.avatar_sm + theme.space_4)
			.rounded(theme.round(theme.avatar_sm + theme.space_4))
			.bg(theme.background_secondary)
			.child(
				Button::new(("friend-more", user.get()))
					.ghost()
					.size_full()
					.rounded(theme.pixels(theme.round(theme.avatar_sm + theme.space_4)))
					.tooltip(cx.t("friends-more"))
					.icon(ComponentIcon::default().path(IconKind::More.path()))
					.dropdown_menu(menu),
			)
			.into_any_element()
	}

	fn message(&mut self, user: Snowflake, cx: &mut Context<Self>) {
		self.data.read(cx).send(Command::OpenDm { user_id: user });
		cx.emit(FriendsEvent::OpenDm);
	}

	pub(super) fn row(
		&self,
		item: &RelationshipRow,
		kind: Kind,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let user = item.user_id;
		let status = Status::from(&item.status);
		let name: SharedString = item.display.clone().into();
		let handle: SharedString = item.username.clone().into();
		let subtitle = self.subtitle(item, kind, status, cx);
		let avatar = media::cdn::avatar_url(user.get(), item.avatar.as_deref(), None, AVATAR_PIXELS);
		let actions = self.actions(item, kind, theme, cx);
		let group: SharedString = format!("friend-row-{user}").into();
		div()
			.id(("friend-row", user.get()))
			.group(group.clone())
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.friends_row_height)
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.border_t_1()
			.border_color(theme.divider)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_hover))
			.on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
				cx.emit(FriendsEvent::OpenProfile {
					user,
					at: event.position(),
				})
			}))
			.child(
				Avatar::new(name.clone())
					.size(theme.avatar_md - theme.space_4)
					.url(avatar)
					.when(kind == Kind::Friend, |avatar| avatar.status(status))
					.cutout(theme.background_primary),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.child(
						div()
							.flex()
							.items_baseline()
							.gap(theme.space_8)
							.child(
								div()
									.truncate()
									.text_size(theme.font_md)
									.font_weight(FontWeight::SEMIBOLD)
									.text_color(theme.header_primary)
									.child(name),
							)
							.child(
								div()
									.truncate()
									.invisible()
									.group_hover(group, |style| style.visible())
									.text_size(theme.font_sm)
									.text_color(theme.header_secondary)
									.child(handle),
							),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(subtitle),
					),
			)
			.child(div().flex().items_center().gap(theme.space_8).children(actions))
			.into_any_element()
	}
}
