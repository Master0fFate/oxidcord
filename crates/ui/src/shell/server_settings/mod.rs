pub mod audit;
pub mod bans;
pub mod channel;
pub mod emoji;
pub mod invites;
pub mod kit;
pub mod members;
pub mod overview;
pub mod rights;
pub mod roles;
pub mod sticker_edit;
pub mod stickers;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
	InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use protocol::{Permissions, Snowflake};

use self::audit::AuditPage;
use self::bans::BansPage;
use self::emoji::EmojiPage;
use self::invites::InvitesPage;
use self::members::MembersPage;
use self::overview::OverviewPage;
use self::roles::RolesPage;
use self::stickers::StickersPage;
use super::community::access::GuildAccess;
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

pub enum ServerSettingsEvent {
	Close,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
	Overview,
	Roles,
	Emoji,
	Stickers,
	Members,
	Invites,
	Bans,
	AuditLog,
}

struct NavGroup {
	title: &'static str,
	pages: &'static [Page],
}

const GROUPS: &[NavGroup] = &[
	NavGroup {
		title: "server-settings-group-server",
		pages: &[Page::Overview, Page::Roles],
	},
	NavGroup {
		title: "server-settings-group-expression",
		pages: &[Page::Emoji, Page::Stickers],
	},
	NavGroup {
		title: "server-settings-group-people",
		pages: &[Page::Members, Page::Invites, Page::Bans],
	},
	NavGroup {
		title: "server-settings-group-moderation",
		pages: &[Page::AuditLog],
	},
];

impl Page {
	fn key(self) -> &'static str {
		match self {
			Self::Overview => "server-settings-overview",
			Self::Roles => "server-settings-roles",
			Self::Emoji => "server-settings-emoji",
			Self::Stickers => "server-settings-stickers",
			Self::Members => "server-settings-members",
			Self::Invites => "server-settings-invites",
			Self::Bans => "server-settings-bans",
			Self::AuditLog => "server-settings-audit-log",
		}
	}

	fn icon(self) -> IconKind {
		match self {
			Self::Overview => IconKind::Settings,
			Self::Roles => IconKind::Shield,
			Self::Emoji => IconKind::Emoji,
			Self::Stickers => IconKind::Sticker,
			Self::Members => IconKind::Members,
			Self::Invites => IconKind::Mail,
			Self::Bans => IconKind::Ban,
			Self::AuditLog => IconKind::AuditLog,
		}
	}

	pub fn allowed(self, access: &GuildAccess) -> bool {
		match self {
			Self::Overview | Self::Invites => access.has(Permissions::MANAGE_GUILD),
			Self::Roles => access.has(Permissions::MANAGE_ROLES),
			Self::Emoji | Self::Stickers => {
				access.has_any(Permissions::MANAGE_GUILD_EXPRESSIONS | Permissions::CREATE_GUILD_EXPRESSIONS)
			}
			Self::Members => access.has_any(
				Permissions::MANAGE_GUILD
					| Permissions::MANAGE_ROLES
					| Permissions::KICK_MEMBERS
					| Permissions::BAN_MEMBERS
					| Permissions::MODERATE_MEMBERS
					| Permissions::MANAGE_NICKNAMES,
			),
			Self::Bans => access.has(Permissions::BAN_MEMBERS),
			Self::AuditLog => access.has(Permissions::VIEW_AUDIT_LOG),
		}
	}
}

enum Body {
	Overview(Entity<OverviewPage>),
	Roles(Entity<RolesPage>),
	Emoji(Entity<EmojiPage>),
	Stickers(Entity<StickersPage>),
	Members(Entity<MembersPage>),
	Invites(Entity<InvitesPage>),
	Bans(Entity<BansPage>),
	Audit(Entity<AuditPage>),
}

pub struct ServerSettings {
	data: Entity<Data>,
	guild: Snowflake,
	page: Page,
	body: Option<Body>,
	focus: FocusHandle,
	_subscription: Subscription,
}

impl EventEmitter<ServerSettingsEvent> for ServerSettings {}

impl Focusable for ServerSettings {
	fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
		self.focus.clone()
	}
}

pub fn first_allowed(access: &GuildAccess) -> Option<Page> {
	GROUPS
		.iter()
		.flat_map(|group| group.pages.iter().copied())
		.find(|page| page.allowed(access))
}

impl ServerSettings {
	pub fn new(
		data: Entity<Data>,
		guild: Snowflake,
		page: Option<Page>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let access = GuildAccess::of(&data.read(cx).state(), guild);
		let page = page
			.filter(|page| page.allowed(&access))
			.or_else(|| first_allowed(&access))
			.unwrap_or(Page::Overview);
		let subscription = cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
			let (DataEvent::Changed(_) | DataEvent::MediaReady) = event;
			cx.notify();
		});
		let focus = cx.focus_handle();
		let mut settings = Self {
			data,
			guild,
			page,
			body: None,
			focus,
			_subscription: subscription,
		};
		settings.select(page, window, cx);
		settings
	}

	fn select(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
		self.page = page;
		let data = self.data.clone();
		let guild = self.guild;
		self.body = Some(match page {
			Page::Overview => Body::Overview(cx.new(|cx| OverviewPage::new(data, guild, window, cx))),
			Page::Roles => Body::Roles(cx.new(|cx| RolesPage::new(data, guild, window, cx))),
			Page::Emoji => Body::Emoji(cx.new(|cx| EmojiPage::new(data, guild, window, cx))),
			Page::Stickers => Body::Stickers(cx.new(|cx| StickersPage::new(data, guild, window, cx))),
			Page::Members => Body::Members(cx.new(|cx| MembersPage::new(data, guild, window, cx))),
			Page::Invites => Body::Invites(cx.new(|cx| InvitesPage::new(data, guild, cx))),
			Page::Bans => Body::Bans(cx.new(|cx| BansPage::new(data, guild, window, cx))),
			Page::AuditLog => Body::Audit(cx.new(|cx| AuditPage::new(data, guild, cx))),
		});
		cx.notify();
	}

	fn nav_item(&self, page: Page, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let active = self.page == page;
		div()
			.id(("server-settings-nav", page as usize))
			.flex()
			.items_center()
			.gap(theme.space_8)
			.h(theme.control_height)
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(theme.font_md)
			.font_weight(FontWeight::MEDIUM)
			.text_color(if active {
				theme.interactive_active
			} else {
				theme.interactive_normal
			})
			.when(active, |item| item.bg(theme.modifier_selected))
			.when(!active, |item| item.hover(|style| style.bg(theme.modifier_hover)))
			.on_click(cx.listener(move |this, _, window, cx| this.select(page, window, cx)))
			.child(Icon::new(page.icon()).size(theme.glyph_md).color(if active {
				theme.interactive_active
			} else {
				theme.interactive_normal
			}))
			.child(cx.t(page.key()))
			.into_any_element()
	}

	fn nav(
		&self,
		access: &GuildAccess,
		name: SharedString,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let groups: Vec<AnyElement> = GROUPS
			.iter()
			.filter_map(|group| {
				let items: Vec<AnyElement> = group
					.pages
					.iter()
					.copied()
					.filter(|page| page.allowed(access))
					.map(|page| self.nav_item(page, theme, cx))
					.collect();
				(!items.is_empty()).then(|| {
					div()
						.flex()
						.flex_col()
						.gap(theme.space_2)
						.pb(theme.space_16)
						.child(
							div()
								.px(theme.space_8)
								.pb(theme.space_4)
								.text_size(theme.font_tiny)
								.font_weight(FontWeight::BOLD)
								.text_color(theme.header_secondary)
								.child(cx.t(group.title).to_uppercase()),
						)
						.children(items)
						.into_any_element()
				})
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.w(theme.settings_nav_width)
			.h_full()
			.flex_none()
			.pt(rem(60.))
			.pr(theme.space_16)
			.child(
				div()
					.px(theme.space_8)
					.pb(theme.space_16)
					.truncate()
					.text_size(theme.font_md)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(name),
			)
			.children(groups)
			.into_any_element()
	}

	fn body_element(&self) -> AnyElement {
		match &self.body {
			Some(Body::Overview(page)) => page.clone().into_any_element(),
			Some(Body::Roles(page)) => page.clone().into_any_element(),
			Some(Body::Emoji(page)) => page.clone().into_any_element(),
			Some(Body::Stickers(page)) => page.clone().into_any_element(),
			Some(Body::Members(page)) => page.clone().into_any_element(),
			Some(Body::Invites(page)) => page.clone().into_any_element(),
			Some(Body::Bans(page)) => page.clone().into_any_element(),
			Some(Body::Audit(page)) => page.clone().into_any_element(),
			None => div().into_any_element(),
		}
	}
}

impl Render for ServerSettings {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (access, name) = {
			let data = self.data.read(cx);
			let state = data.state();
			(
				GuildAccess::of(&state, self.guild),
				state
					.guild(self.guild)
					.map(|guild| guild.name.clone())
					.unwrap_or_default(),
			)
		};
		let viewport = window.viewport_size().width;
		let nav_width = theme.pixels(theme.settings_nav_width);
		let space = theme.pixels(theme.space_32);
		let content_width = theme
			.pixels(theme.settings_content_width)
			.min(viewport - nav_width - space * 4.);
		let column = content_width + space * 3.;
		let left = ((viewport - nav_width - column) / 2.).max(px(0.)) + nav_width;
		let nav = self.nav(&access, name.into(), &theme, cx);
		div()
			.id("server-settings")
			.track_focus(&self.focus)
			.occlude()
			.absolute()
			.top(theme.title_bar_height)
			.left_0()
			.right_0()
			.bottom_0()
			.flex()
			.justify_center()
			.bg(theme.background_primary)
			.text_color(theme.text_normal)
			.child(
				div()
					.flex()
					.flex_none()
					.justify_end()
					.w(left)
					.h_full()
					.bg(theme.background_secondary)
					.child(nav),
			)
			.child(
				div()
					.relative()
					.flex()
					.flex_col()
					.flex_1()
					.h_full()
					.min_w_0()
					.overflow_hidden()
					.child(
						div()
							.flex()
							.flex_col()
							.w(content_width)
							.h_full()
							.min_h_0()
							.pt(rem(60.))
							.pl(theme.space_32)
							.pr(theme.space_32 * 2.)
							.child(self.body_element()),
					)
					.child(
						div().absolute().top(rem(60.)).right(theme.space_16).child(
							div()
								.id("server-settings-close")
								.flex()
								.items_center()
								.justify_center()
								.size(rem(36.))
								.rounded(theme.round(rem(36.)))
								.border_2()
								.border_color(theme.interactive_normal)
								.cursor_pointer()
								.hover(|style| {
									style
										.bg(theme.modifier_hover)
										.border_color(theme.interactive_hover)
								})
								.on_click(cx.listener(|_, _, _, cx| cx.emit(ServerSettingsEvent::Close)))
								.child(
									Icon::new(IconKind::Close)
										.size(theme.glyph_md)
										.color(theme.interactive_normal),
								),
						),
					),
			)
	}
}
