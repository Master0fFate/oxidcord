use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use gpui::{
	App, AppContext as _, Bounds, Context, Entity, Global, IntoElement, ParentElement as _, Render,
	Styled as _, Subscription, Task, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions, div,
	point, px, size,
};
use gpui_component::Root;
use media::{Limits, MediaFetcher, MediaService};
use protocol::Snowflake;

use session::{Command, NotifyFn, Session};
use state::Changes;
use store::{Settings, Store};
use tokio::sync::mpsc::UnboundedReceiver;
use url::Url;

use crate::actions;
use crate::data::{Data, DataEvent};
use crate::desktop::{self, Desktop, DesktopEvent, TrayActive};
use crate::i18n::Localizer;
use crate::images::Images;
use crate::playback::Playback;
use crate::shell::{
	DataDir, LoginEvent, LoginOptions, LoginScreen, PluginStore, SettingsBackend, Shell, ShellEvent,
	Textures, UpdateService, VideoTextures, VoiceHotkeys, set_reduced_motion,
};
use crate::theme::{SYSTEM_THEME, Theme};
use crate::zoom;

pub(crate) struct AppHandle(WeakEntity<AppRoot>);

impl Global for AppHandle {}

const APP_TITLE: &str = "Oxidcord";
const MIN_WIDTH: f32 = 940.;
const MIN_HEIGHT: f32 = 500.;
const TICK_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Default)]
pub struct AppFlags {
	pub mock: bool,
	pub safe_mode: bool,
}

impl Global for AppFlags {}

pub type StartSession = Box<dyn Fn(String, NotifyFn) -> Session>;

pub struct LaunchConfig {
	pub store: Arc<Store>,
	pub flags: AppFlags,
	pub rest_base: Url,
	pub mock_origin: Option<Url>,
	pub token: Option<String>,
	pub start_session: StartSession,
	pub qr_config: login::QrConfig,
	pub web_login_helper: Option<PathBuf>,
	pub activation: Option<Receiver<Vec<u8>>>,
	pub on_window_ready: Option<Box<dyn FnOnce()>>,
}

enum Signal {
	Changes(Box<Changes>),
	Media,
}

enum Screen {
	Login(Entity<LoginScreen>),
	Main(Entity<Shell>),
}

pub(crate) struct AppRoot {
	store: Arc<Store>,
	backend: Rc<SettingsBackend>,
	rest_base: Url,
	save_token: bool,
	start_session: StartSession,
	qr_config: login::QrConfig,
	web_login_helper: Option<PathBuf>,
	data: Option<Entity<Data>>,
	_desktop: Option<Desktop>,
	media: MediaService,
	screen: Screen,
	pump: Option<Task<()>>,
	auto_idle: bool,
	_ticker: Option<Task<()>>,
	_activation: Option<Subscription>,
	_screen_subscription: Option<Subscription>,
	_switch_subscription: Option<Subscription>,
}

pub fn run(mut config: LaunchConfig) {
	gpui_platform::application()
		.with_assets(gpui_kit_assets::AllAssets)
		.run(move |cx| {
			gpui_component::init(cx);
			actions::bind_keys(cx);
			let settings = config.store.settings.read().clone();
			Localizer::new(&settings.locale).install(cx);
			Theme::set_font_scale(settings.accessibility.font_scale, cx);
			Theme::resolve(&settings.theme, &config.store.paths.themes_dir()).install(cx);
			zoom::install(config.store.clone(), cx);
			set_reduced_motion(settings.accessibility.reduced_motion);
			cx.set_global(config.flags);
			cx.set_global(Textures(VideoTextures::new()));
			VoiceHotkeys::install(&settings.keybinds, cx);
			cx.on_window_closed(|cx, _| {
				if cx.windows().is_empty() {
					cx.quit();
				}
			})
			.detach();
			let media = match media_service(&config) {
				Ok(media) => media,
				Err(error) => {
					tracing::error!(%error, "could not start the media service");
					cx.quit();
					return;
				}
			};
			cx.set_global(Images::new(media.clone(), config.mock_origin.clone()));
			Playback::install(
				media.clone(),
				config.mock_origin.clone(),
				config.store.paths.cache_dir().join("embeds"),
				cx,
			);
			UpdateService::launch(
				config.store.clone(),
				settings.updates.check_on_start && !config.flags.mock,
				cx,
			);
			cx.set_global(PluginStore(config.store.clone()));
			cx.set_global(DataDir(config.store.paths.data_dir().to_path_buf()));
			let store = config.store.clone();
			let on_window_ready = config.on_window_ready.take();
			let backend = settings_backend(&store);
			let options = WindowOptions {
				window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
					None,
					size(
						px(settings.window.width as f32),
						px(settings.window.height as f32),
					),
					cx,
				))),
				window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
				titlebar: Some(TitlebarOptions {
					title: Some(APP_TITLE.into()),
					appears_transparent: true,
					traffic_light_position: Some(point(px(9.), px(9.))),
				}),
				app_owns_titlebar_drag: true,
				..WindowOptions::default()
			};
			let window = cx
				.open_window(options, move |window, cx| {
					zoom::attach(window, cx);
					let root = cx.new(|cx| AppRoot::new(config, media, backend, window, cx));
					cx.new(|cx| Root::new(root, window, cx))
				})
				.expect("open window");
			window
				.update(cx, |_, window, cx| {
					window.on_window_should_close(cx, move |window, cx| {
						remember_window(&store, window);
						let to_tray =
							store.settings.read().window.minimize_to_tray && cx.has_global::<TrayActive>();
						!(to_tray && desktop::hide_window(window))
					});
				})
				.expect("window is open");
			cx.activate(true);
			if let Some(ready) = on_window_ready {
				ready();
			}
		});
}

fn media_service(config: &LaunchConfig) -> media::Result<MediaService> {
	let chrome_version = config.store.settings.read().client.chrome_version.clone();
	let mut fetcher =
		MediaFetcher::builder().user_agent(protocol::gateway::chrome_user_agent(&chrome_version));
	if let Some(port) = config.mock_origin.as_ref().and_then(Url::port) {
		fetcher = fetcher.allow_loopback_http(port);
	}
	Ok(MediaService::new(
		fetcher.build()?,
		config.store.paths.cache_dir().join("media"),
		Limits::default(),
	))
}

fn fold(signal: Signal, changes: &mut Changes, media: &mut bool) {
	match signal {
		Signal::Changes(next) => changes.merge(*next),
		Signal::Media => *media = true,
	}
}

fn pump(
	data: WeakEntity<Data>,
	mut receiver: UnboundedReceiver<Signal>,
	cx: &mut Context<AppRoot>,
) -> Task<()> {
	cx.spawn(async move |_, cx| {
		while let Some(first) = receiver.recv().await {
			let mut changes = Changes::default();
			let mut media = false;
			fold(first, &mut changes, &mut media);
			while let Ok(next) = receiver.try_recv() {
				fold(next, &mut changes, &mut media);
			}
			let delivered = data.update(cx, |data, cx| {
				if !changes.is_empty() {
					data.publish(DataEvent::Changed(Box::new(changes)), cx);
				}
				if media {
					data.publish(DataEvent::MediaReady, cx);
				}
			});
			if delivered.is_err() {
				break;
			}
		}
	})
}

impl AppRoot {
	fn new(
		config: LaunchConfig,
		media: MediaService,
		backend: Rc<SettingsBackend>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let options = LoginOptions {
			rest_base: config.rest_base.clone(),
			qr_config: config.qr_config.clone(),
			web_helper: config.web_login_helper.clone(),
		};
		let login = cx.new(|cx| LoginScreen::new(options, backend.clone(), window, cx));
		let mut root = Self {
			store: config.store,
			backend,
			rest_base: config.rest_base,
			save_token: !config.flags.mock,
			start_session: config.start_session,
			qr_config: config.qr_config,
			web_login_helper: config.web_login_helper,
			data: None,
			_desktop: Some(Desktop::install(config.activation, window, cx)),
			media,
			screen: Screen::Login(login),
			pump: None,
			auto_idle: false,
			_ticker: None,
			_activation: None,
			_screen_subscription: None,
			_switch_subscription: None,
		};
		let handle = AppHandle(cx.weak_entity());
		cx.set_global(handle);
		root._activation = Some(cx.observe_window_activation(window, |this, window, cx| {
			if window.is_window_active() {
				this.follow_system_theme(cx);
			}
		}));
		root._ticker = Some(cx.spawn(async move |this, cx| {
			loop {
				cx.background_executor().timer(TICK_INTERVAL).await;
				if this.update(cx, |this, cx| this.tick(cx)).is_err() {
					break;
				}
			}
		}));
		match config.token {
			Some(token) => root.open_session(&token, window, cx),
			None => root.show_login(window, cx),
		}
		root
	}

	pub(crate) fn add_account(window: &mut Window, cx: &mut App) {
		let Some(root) = cx.try_global::<AppHandle>().and_then(|handle| handle.0.upgrade()) else {
			return;
		};
		root.update(cx, |root, cx| root.show_login(window, cx));
	}

	fn show_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.pump = None;
		self.data = None;
		self._switch_subscription = None;
		let options = LoginOptions {
			rest_base: self.rest_base.clone(),
			qr_config: self.qr_config.clone(),
			web_helper: self.web_login_helper.clone(),
		};
		let backend = self.backend.clone();
		let login = cx.new(|cx| LoginScreen::new(options, backend, window, cx));
		self._screen_subscription =
			Some(
				cx.subscribe_in(&login, window, |this, _, event: &LoginEvent, window, cx| {
					let LoginEvent::Authenticated(token) = event;
					if this.save_token
						&& let Err(error) =
							login::persist::save_token(&this.store, &protocol::Token::new(token.clone()))
					{
						tracing::warn!(%error, "could not save the token");
					}
					this.open_session(token, window, cx);
				}),
			);
		self.screen = Screen::Login(login);
		window.set_window_title(APP_TITLE);
		cx.notify();
	}

	fn open_session(&mut self, token: &str, window: &mut Window, cx: &mut Context<Self>) {
		let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
		let notify_sender = sender.clone();
		let notify: NotifyFn = Arc::new(move |changes| {
			let _ = notify_sender.send(Signal::Changes(Box::new(changes)));
		});
		self.media.set_on_ready(move || {
			let _ = sender.send(Signal::Media);
		});
		let session = (self.start_session)(token.to_owned(), notify);
		session.set_video_sink(cx.global::<Textures>().0.clone());
		let data = cx.new(|_| Data::new(session));
		VoiceHotkeys::attach(data.downgrade(), cx);
		self._switch_subscription =
			Some(
				cx.subscribe_in(&data, window, |_, data, event: &DataEvent, window, cx| {
					let DataEvent::Changed(changes) = event else {
						return;
					};
					if !changes.session {
						return;
					}
					let Some(user_id) = data.read(cx).state().account_switch_requested() else {
						return;
					};
					cx.spawn_in(window, async move |this, cx| {
						this.update_in(cx, |this, window, cx| this.switch_account(user_id, window, cx))
							.ok();
					})
					.detach();
				}),
			);
		self.data = Some(data.clone());
		self.pump = Some(pump(data.downgrade(), receiver, cx));
		let backend = self.backend.clone();
		let shell = cx.new(|cx| Shell::new(data, backend, window, cx));
		self._screen_subscription =
			Some(
				cx.subscribe_in(&shell, window, |this, _, event: &ShellEvent, window, cx| {
					let ShellEvent::LoggedOut = event;
					this.show_login(window, cx);
				}),
			);
		self.screen = Screen::Main(shell);
		cx.notify();
	}

	fn switch_account(&mut self, user_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		let token = match self.store.accounts.token_for(user_id) {
			Ok(Some(token)) => token,
			Ok(None) => return,
			Err(error) => {
				tracing::warn!(%error, "could not read the account sign-in");
				return;
			}
		};
		if self.save_token
			&& let Err(error) = login::persist::save_token(&self.store, &protocol::Token::new(token.clone()))
		{
			tracing::warn!(%error, "could not save the token");
		}
		if let Err(error) = self.store.accounts.set_current(user_id) {
			tracing::warn!(%error, "could not mark the account as current");
		}
		self.pump = None;
		self.data = None;
		self.open_session(&token, window, cx);
	}

	fn tick(&mut self, cx: &mut Context<Self>) {
		self.follow_system_theme(cx);
		self.sync_idle_presence(cx);
	}

	fn follow_system_theme(&self, cx: &mut Context<Self>) {
		if self.store.settings.read().theme != SYSTEM_THEME {
			return;
		}
		let next = Theme::system();
		if cx.global::<Theme>().name != next.name {
			next.install(cx);
		}
	}

	fn sync_idle_presence(&mut self, cx: &mut Context<Self>) {
		let Some(data) = &self.data else {
			return;
		};
		let limit = u64::from(self.store.settings.read().presence.idle_after_secs);
		let away = limit > 0 && platform::idle::seconds_since_input().is_some_and(|secs| secs >= limit);
		if away == self.auto_idle {
			return;
		}
		self.auto_idle = away;
		data.read(cx).send(Command::SetAfk(away));
	}

	pub(crate) fn on_desktop(&mut self, event: DesktopEvent, window: &mut Window, cx: &mut Context<Self>) {
		match event {
			DesktopEvent::Show => desktop::show_window(window),
			DesktopEvent::ToggleMute => {
				if let Some(data) = &self.data {
					data.update(cx, |data, cx| data.toggle_mute(cx));
				}
			}
			DesktopEvent::ToggleDeafen => {
				if let Some(data) = &self.data {
					data.update(cx, |data, cx| data.toggle_deafen(cx));
				}
			}
			DesktopEvent::Quit => {
				remember_window(&self.store, window);
				cx.quit();
			}
		}
	}
}

impl Render for AppRoot {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		let screen = match &self.screen {
			Screen::Login(login) => login.clone().into_any_element(),
			Screen::Main(shell) => shell.clone().into_any_element(),
		};
		div().size_full().child(screen)
	}
}

fn settings_backend(store: &Arc<Store>) -> Rc<SettingsBackend> {
	let reader = store.clone();
	let writer = store.clone();
	Rc::new(SettingsBackend {
		read: Box::new(move || reader.settings.read().clone()),
		write: Box::new(move |apply| {
			if let Err(error) = writer.update_settings(|settings: &mut Settings| apply(settings)) {
				tracing::warn!(%error, "could not save settings");
			}
		}),
		themes_dir: store.paths.themes_dir(),
	})
}

fn remember_window(store: &Store, window: &gpui::Window) {
	let (bounds, maximized) = match window.window_bounds() {
		WindowBounds::Windowed(bounds) => (bounds, false),
		WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, true),
	};
	let result = store.update_settings(|settings| {
		settings.window.maximized = maximized;
		if !maximized {
			settings.window.width = f32::from(bounds.size.width).round() as u32;
			settings.window.height = f32::from(bounds.size.height).round() as u32;
		}
	});
	if let Err(error) = result {
		tracing::warn!(%error, "could not save window size");
	}
}
