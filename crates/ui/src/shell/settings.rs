use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
	InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::radio::{Radio, RadioGroup};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use store::Settings;

use super::account::{AccountPages, Note, PageEvent, Target, chat_page, note_page, streamer_page};
use super::plugins::{PluginPages, ThemeApplied};
use super::updates::UpdatesPage;
use super::voice::{KeybindsPage, VoiceVideoPage};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};
use crate::zoom;

const NAV_WIDTH: f32 = 218.;
const CONTENT_WIDTH: f32 = 740.;
const THEMES: [&str; 4] = ["dark", "light", "amoled", "system"];

pub type SettingsWriter = Box<dyn Fn(&dyn Fn(&mut Settings))>;

pub struct SettingsBackend {
	pub read: Box<dyn Fn() -> Settings>,
	pub write: SettingsWriter,
	pub themes_dir: PathBuf,
}

pub enum SettingsEvent {
	Close,
	CompactMessages(bool),
	LogOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Page {
	MyAccount,
	Profiles,
	Privacy,
	Devices,
	Connections,
	Clips,
	FriendRequests,
	Appearance,
	Accessibility,
	VoiceVideo,
	Chat,
	Notifications,
	Keybinds,
	Language,
	StreamerMode,
	Advanced,
	Plugins,
	Themes,
	Developer,
	Updates,
	PluginSlot,
}

impl Page {
	fn key(self) -> &'static str {
		match self {
			Self::MyAccount => "settings-my-account",
			Self::Profiles => "settings-profiles",
			Self::Privacy => "settings-privacy",
			Self::Devices => "settings-devices",
			Self::Connections => "settings-connections",
			Self::Clips => "settings-clips",
			Self::FriendRequests => "settings-friend-requests",
			Self::Appearance => "settings-appearance",
			Self::Accessibility => "settings-accessibility",
			Self::VoiceVideo => "settings-voice-video",
			Self::Chat => "settings-chat",
			Self::Notifications => "settings-notifications",
			Self::Keybinds => "settings-keybinds",
			Self::Language => "settings-language",
			Self::StreamerMode => "settings-streamer-mode",
			Self::Advanced => "settings-advanced",
			Self::Plugins => "settings-plugins",
			Self::Themes => "settings-themes",
			Self::Developer => "settings-developer",
			Self::Updates => "settings-updates",
			Self::PluginSlot => "settings-plugins",
		}
	}

	fn icon(self) -> IconKind {
		match self {
			Self::MyAccount => IconKind::Account,
			Self::Profiles => IconKind::Profile,
			Self::Privacy => IconKind::Shield,
			Self::Devices => IconKind::Devices,
			Self::Connections => IconKind::Connections,
			Self::Clips => IconKind::Clips,
			Self::FriendRequests => IconKind::FriendRequests,
			Self::Appearance => IconKind::Appearance,
			Self::Accessibility => IconKind::Accessibility,
			Self::VoiceVideo => IconKind::Mic,
			Self::Chat => IconKind::Chat,
			Self::Notifications => IconKind::Bell,
			Self::Keybinds => IconKind::Keybinds,
			Self::Language => IconKind::Language,
			Self::StreamerMode => IconKind::Streamer,
			Self::Advanced => IconKind::Advanced,
			Self::Plugins => IconKind::Plugins,
			Self::Themes => IconKind::Themes,
			Self::Developer => IconKind::Developer,
			Self::Updates => IconKind::Download,
			Self::PluginSlot => IconKind::Plugins,
		}
	}
}

const SECTIONS: [(Option<&str>, &[Page]); 3] = [
	(
		None,
		&[
			Page::MyAccount,
			Page::Profiles,
			Page::Privacy,
			Page::Devices,
			Page::Connections,
			Page::Clips,
			Page::FriendRequests,
		],
	),
	(
		Some("settings-section-app"),
		&[
			Page::Appearance,
			Page::Accessibility,
			Page::VoiceVideo,
			Page::Chat,
			Page::Notifications,
			Page::Keybinds,
			Page::Language,
			Page::StreamerMode,
			Page::Advanced,
		],
	),
	(
		Some("settings-section-oxidcord"),
		&[Page::Plugins, Page::Themes, Page::Developer, Page::Updates],
	),
];

pub struct SettingsWindow {
	focus: FocusHandle,
	page: Page,
	settings: Settings,
	backend: Rc<SettingsBackend>,
	data: Entity<Data>,
	voice_video: Option<Entity<VoiceVideoPage>>,
	keybinds: Option<Entity<KeybindsPage>>,
	account_pages: AccountPages,
	plugin_pages: PluginPages,
	updates: Entity<UpdatesPage>,
	ui_scale: Entity<SliderState>,
}

impl EventEmitter<SettingsEvent> for SettingsWindow {}

impl Focusable for SettingsWindow {
	fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
		self.focus.clone()
	}
}

impl SettingsWindow {
	pub fn new(
		settings: Settings,
		backend: Rc<SettingsBackend>,
		data: Entity<Data>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let account_pages = AccountPages::new(data.clone(), backend.clone(), window, cx);
		cx.subscribe_in(
			&account_pages.my_account,
			window,
			|this, _, event: &PageEvent, window, cx| this.on_account_page(event, window, cx),
		)
		.detach();
		let plugin_pages = PluginPages::new(window, cx);
		cx.subscribe(&plugin_pages.themes, |this, _, event: &ThemeApplied, cx| {
			this.settings.theme.clone_from(&event.0);
			cx.notify();
		})
		.detach();
		let updates = cx.new(|cx| UpdatesPage::new(backend.clone(), cx));
		UpdatesPage::watch(cx);
		let ui_scale = cx.new(|cx| {
			SliderState::new()
				.min(zoom::MIN * 100.)
				.max(zoom::MAX * 100.)
				.step(zoom::STEP * 100.)
				.default_value((zoom::current(cx) * 100.).round())
		});
		cx.subscribe(&ui_scale, |_, _, event: &SliderEvent, cx| match event {
			SliderEvent::Change(value) => zoom::apply(value.end() / 100., cx),
			SliderEvent::Release(value) => zoom::persist(value.end() / 100., cx),
		})
		.detach();
		cx.observe_global_in::<Theme>(window, |this, window, cx| this.sync_ui_scale(window, cx))
			.detach();
		let focus = cx.focus_handle();
		window.focus(&focus, cx);
		Self {
			focus,
			page: Page::Appearance,
			settings,
			backend,
			data,
			voice_video: None,
			keybinds: None,
			plugin_pages,
			account_pages,
			updates,
			ui_scale,
		}
	}

	fn sync_ui_scale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let percent = (zoom::current(cx) * 100.).round();
		if (self.ui_scale.read(cx).value().end() - percent).abs() > 0.5 {
			self.ui_scale
				.update(cx, |slider, cx| slider.set_value(percent, window, cx));
		}
		cx.notify();
	}

	pub fn show_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.show_page(Page::Profiles, window, cx);
	}

	fn on_account_page(&mut self, event: &PageEvent, window: &mut Window, cx: &mut Context<Self>) {
		match event {
			PageEvent::Goto(Target::Profiles) => self.show_page(Page::Profiles, window, cx),
			PageEvent::LogOut => cx.emit(SettingsEvent::LogOut),
		}
	}

	pub fn show_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.show_page(Page::VoiceVideo, window, cx);
	}

	fn show_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
		match page {
			Page::VoiceVideo if self.voice_video.is_none() => {
				let (data, backend) = (self.data.clone(), self.backend.clone());
				self.voice_video = Some(cx.new(|cx| VoiceVideoPage::new(data, backend, window, cx)));
			}
			Page::Keybinds if self.keybinds.is_none() => {
				let backend = self.backend.clone();
				self.keybinds = Some(cx.new(|cx| KeybindsPage::new(backend, cx)));
			}
			Page::Devices => self
				.account_pages
				.devices
				.update(cx, |devices, cx| devices.refresh(cx)),
			_ => {}
		}
		self.page = page;
		cx.notify();
	}

	fn write(&mut self, apply: impl Fn(&mut Settings) + Clone) {
		apply(&mut self.settings);
		(self.backend.write)(&apply);
	}

	fn set_theme(&mut self, name: &'static str, cx: &mut Context<Self>) {
		self.write(move |settings| settings.theme = name.to_owned());
		Theme::resolve(name, &self.backend.themes_dir).install(cx);
		cx.notify();
	}

	fn set_compact(&mut self, compact: bool, cx: &mut Context<Self>) {
		self.write(move |settings| settings.appearance.compact_messages = compact);
		cx.emit(SettingsEvent::CompactMessages(compact));
		cx.notify();
	}

	fn set_share_gif_usage(&mut self, share: bool, cx: &mut Context<Self>) {
		self.write(move |settings| settings.advanced.share_gif_usage = share);
		cx.notify();
	}

	fn advanced(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		div()
			.flex()
			.flex_col()
			.child(self.label("settings-gif-usage", theme, cx))
			.child(
				Switch::new("settings-gif-usage")
					.checked(self.settings.advanced.share_gif_usage)
					.label(cx.t("settings-gif-usage-switch"))
					.on_change(
						cx.listener(|this, checked: &bool, _, cx| this.set_share_gif_usage(*checked, cx)),
					),
			)
			.child(
				div()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t("settings-gif-usage-hint")),
			)
			.into_any_element()
	}

	fn nav_item(&self, page: Page, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let selected = self.page == page;
		let (rest, hover) = if selected {
			(theme.interactive_active, theme.interactive_active)
		} else {
			(theme.interactive_normal, theme.interactive_hover)
		};
		let group: SharedString = format!("settings-nav-{}", page.key()).into();
		div()
			.id(page.key())
			.group(group.clone())
			.flex()
			.items_center()
			.gap(theme.space_8)
			.h(rem(32.))
			.px(rem(10.))
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(theme.font_md)
			.font_weight(FontWeight::MEDIUM)
			.text_color(rest)
			.when(selected, |item| item.bg(theme.modifier_selected))
			.when(!selected, |item| {
				item.hover(|style| style.bg(theme.modifier_hover).text_color(hover))
			})
			.on_click(cx.listener(move |this, _, window, cx| this.show_page(page, window, cx)))
			.child(
				Icon::new(page.icon())
					.size(rem(18.))
					.color(rest)
					.group_hover(group, hover),
			)
			.child(div().min_w_0().truncate().child(cx.t(page.key())))
			.children(if page == Page::Updates {
				UpdatesPage::ready_badge(theme, cx)
			} else {
				None
			})
			.into_any_element()
	}

	fn nav(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let mut items: Vec<AnyElement> = Vec::new();
		for (heading, pages) in SECTIONS {
			if let Some(key) = heading {
				items.push(
					div()
						.px(rem(10.))
						.pt(theme.space_16)
						.pb(theme.space_4)
						.text_size(theme.font_tiny)
						.font_weight(FontWeight::BOLD)
						.text_color(theme.header_secondary)
						.child(cx.t(key).to_uppercase())
						.into_any_element(),
				);
			}
			for page in pages {
				items.push(self.nav_item(*page, theme, cx));
			}
			if heading.is_none() {
				items.push(div().h(theme.space_8).into_any_element());
			}
		}
		items.extend(
			self.plugin_pages
				.nav_items(self.page == Page::PluginSlot, theme, cx, |this, cx| {
					this.page = Page::PluginSlot;
					cx.notify();
				}),
		);
		let logout_color = theme.danger;
		items.push(
			div()
				.mt(theme.space_8)
				.mb(theme.space_8)
				.mx(rem(10.))
				.h(rem(1.))
				.bg(theme.divider)
				.into_any_element(),
		);
		items.push(
			div()
				.id("settings-log-out")
				.flex()
				.items_center()
				.gap(theme.space_8)
				.h(rem(32.))
				.px(rem(10.))
				.rounded(theme.radius_sm)
				.cursor_pointer()
				.text_size(theme.font_md)
				.font_weight(FontWeight::MEDIUM)
				.text_color(theme.interactive_normal)
				.hover(|style| style.bg(theme.modifier_hover))
				.on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::LogOut)))
				.child(Icon::new(IconKind::LogOut).size(rem(18.)).color(logout_color))
				.child(cx.t("settings-log-out"))
				.into_any_element(),
		);
		items.push(
			div()
				.mx(rem(10.))
				.mt(theme.space_8)
				.text_size(theme.font_tiny)
				.text_color(theme.text_muted)
				.child(cx.t_args(
					"settings-version",
					&[("version", ArgValue::from(env!("CARGO_PKG_VERSION")))],
				))
				.into_any_element(),
		);
		div()
			.flex()
			.flex_col()
			.children(items)
			.overflow_y_scrollbar()
			.id("settings-nav")
			.w(rem(NAV_WIDTH))
			.h_full()
			.pt(rem(60.))
			.pl(theme.space_20)
			.pr(rem(6.))
			.pb(theme.space_32)
			.into_any_element()
	}

	fn label(&self, key: &str, theme: &Theme, cx: &Context<Self>) -> impl IntoElement {
		div()
			.mb(theme.space_8)
			.text_size(theme.font_tiny)
			.font_weight(FontWeight::BOLD)
			.text_color(theme.header_secondary)
			.child(cx.t(key).to_uppercase())
	}

	fn dpi_scale(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
		let percent = self.ui_scale.read(cx).value().end().round();
		div()
			.flex()
			.flex_col()
			.child(self.label("settings-dpi-scale", theme, cx))
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_16)
					.child(div().flex_1().child(Slider::new(&self.ui_scale)))
					.child(
						div()
							.w(rem(48.))
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(format!("{percent}%")),
					)
					.child(
						Button::new("settings-dpi-reset")
							.ghost()
							.small()
							.label(cx.t("settings-reset"))
							.on_click(cx.listener(|_, _, _, cx| zoom::set(1., cx))),
					),
			)
			.child(
				div()
					.mt(theme.space_8)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t("settings-dpi-hint")),
			)
	}

	fn appearance(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let selected_theme = THEMES.iter().position(|name| *name == self.settings.theme);
		let compact = self.settings.appearance.compact_messages;
		let theme_labels = [
			cx.t("settings-theme-dark"),
			cx.t("settings-theme-light"),
			cx.t("settings-theme-amoled"),
			cx.t("settings-theme-system"),
		];
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.child(
				div()
					.flex()
					.flex_col()
					.child(self.label("settings-theme", theme, cx))
					.child(
						RadioGroup::vertical("settings-theme-group")
							.selected_index(selected_theme)
							.children(
								theme_labels
									.into_iter()
									.enumerate()
									.map(|(index, label)| Radio::new(("settings-theme", index)).label(label)),
							)
							.on_change(cx.listener(|this, index: &usize, _, cx| {
								if let Some(name) = THEMES.get(*index) {
									this.set_theme(name, cx);
								}
							})),
					),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.child(self.label("settings-message-display", theme, cx))
					.child(
						RadioGroup::vertical("settings-display-group")
							.selected_index(Some(usize::from(compact)))
							.child(Radio::new("settings-display-cozy").label(cx.t("settings-display-cozy")))
							.child(
								Radio::new("settings-display-compact")
									.label(cx.t("settings-display-compact")),
							)
							.on_change(
								cx.listener(|this, index: &usize, _, cx| this.set_compact(*index == 1, cx)),
							),
					),
			)
			.child(self.dpi_scale(theme, cx))
			.into_any_element()
	}

	fn content(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let body = match self.page {
			Page::Appearance => self.appearance(theme, cx),
			Page::Advanced => div()
				.flex()
				.flex_col()
				.gap(theme.space_24)
				.child(self.account_pages.advanced.clone())
				.child(self.advanced(theme, cx))
				.into_any_element(),
			Page::MyAccount => self.account_pages.my_account.clone().into_any_element(),
			Page::Profiles => self.account_pages.profiles.clone().into_any_element(),
			Page::Language => self.account_pages.language.clone().into_any_element(),
			Page::Notifications => self.account_pages.notifications.clone().into_any_element(),
			Page::StreamerMode => streamer_page(&self.backend, cx),
			Page::Chat => chat_page(&self.backend, cx),
			Page::Accessibility => self.account_pages.accessibility.clone().into_any_element(),
			Page::FriendRequests => self.account_pages.friend_requests.clone().into_any_element(),
			Page::Devices => self.account_pages.devices.clone().into_any_element(),
			Page::Privacy => note_page(Note::Privacy, cx),
			Page::Connections => note_page(Note::Connections, cx),
			Page::Clips => note_page(Note::Clips, cx),
			Page::VoiceVideo => div().children(self.voice_video.clone()).into_any_element(),
			Page::Keybinds => div().children(self.keybinds.clone()).into_any_element(),
			Page::Plugins => self.plugin_pages.plugins.clone().into_any_element(),
			Page::Themes => self.plugin_pages.themes.clone().into_any_element(),
			Page::Developer => self.plugin_pages.developer.clone().into_any_element(),
			Page::Updates => self.updates.clone().into_any_element(),
			Page::PluginSlot => self.plugin_pages.slot.clone().into_any_element(),
		};
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.child(
				div()
					.text_size(theme.font_lg)
					.line_height(rem(24.))
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(cx.t(self.page.key())),
			)
			.child(body)
			.into_any_element()
	}
}

impl Render for SettingsWindow {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let viewport = window.viewport_size().width;
		let right = theme
			.pixels(rem(CONTENT_WIDTH) + rem(140.))
			.min(viewport - theme.pixels(rem(NAV_WIDTH + 40.)));
		let left = viewport - right;
		let content = self.content(&theme, cx);
		let nav = self.nav(&theme, cx);
		div()
			.id("settings-window")
			.track_focus(&self.focus)
			.occlude()
			.absolute()
			.top(theme.title_bar_height)
			.left_0()
			.right_0()
			.bottom_0()
			.flex()
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
					.flex_none()
					.w(right)
					.h_full()
					.child(
						div()
							.flex()
							.flex_col()
							.max_w(rem(CONTENT_WIDTH))
							.h_full()
							.pt(rem(60.))
							.pl(theme.space_32)
							.pr(theme.space_16)
							.pb(theme.space_32)
							.child(content)
							.overflow_y_scrollbar()
							.id("settings-content"),
					)
					.child(
						div()
							.absolute()
							.top(rem(60.))
							.right(theme.space_16)
							.flex()
							.flex_col()
							.items_center()
							.gap(theme.space_4)
							.child(
								div()
									.id("settings-close")
									.flex()
									.items_center()
									.justify_center()
									.size(rem(36.))
									.rounded_full()
									.border_2()
									.border_color(theme.interactive_normal)
									.cursor_pointer()
									.hover(|style| {
										style
											.bg(theme.modifier_hover)
											.border_color(theme.interactive_hover)
									})
									.on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close)))
									.child(
										Icon::new(IconKind::Close)
											.size(rem(18.))
											.color(theme.interactive_normal),
									),
							)
							.child(
								div()
									.text_size(theme.font_xs)
									.font_weight(FontWeight::BOLD)
									.text_color(theme.interactive_normal)
									.child(cx.t("settings-close-hint")),
							),
					),
			)
	}
}
