use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ObjectFit,
	ParentElement as _, Render, SharedString, Styled as _, StyledImage as _, Subscription, Window, div, img,
	prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{
	ChannelType, DefaultMessageNotifications, ExplicitContentFilter, Snowflake, SystemChannelFlags,
	VerificationLevel,
};
use session::{Command, GuildEdit};
use state::GuildState;

use super::{kit, transfer};
use crate::components::{
	Avatar, PickHandler, ToggleHandler, dialog_footer, field, hint, segmented, select, switch_row,
};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::shell::community::data_handle;
use crate::shell::community::files::{Fit, IMAGE_TYPES, Picked, pick_image};
use crate::shell::community::modal::{self, Close};
use crate::theme::{Theme, rem};

const ICON_SIDE: u32 = 512;
const BANNER_SIZE: (u32, u32) = (960, 540);
const SPLASH_SIZE: (u32, u32) = (1920, 1080);
const NAME_LIMIT: usize = 100;

const AFK_TIMEOUTS: [(&str, u32); 5] = [
	("afk-timeout-1m", 60),
	("afk-timeout-5m", 300),
	("afk-timeout-15m", 900),
	("afk-timeout-30m", 1800),
	("afk-timeout-1h", 3600),
];
const NOTIFICATIONS: [(&str, DefaultMessageNotifications); 2] = [
	("notify-all", DefaultMessageNotifications::AllMessages),
	("notify-mentions", DefaultMessageNotifications::OnlyMentions),
];
const VERIFICATIONS: [(&str, VerificationLevel); 5] = [
	("verification-none", VerificationLevel::None),
	("verification-low", VerificationLevel::Low),
	("verification-medium", VerificationLevel::Medium),
	("verification-high", VerificationLevel::High),
	("verification-highest", VerificationLevel::VeryHigh),
];
const FILTERS: [(&str, ExplicitContentFilter); 3] = [
	("filter-off", ExplicitContentFilter::Disabled),
	("filter-roleless", ExplicitContentFilter::MembersWithoutRoles),
	("filter-all", ExplicitContentFilter::AllMembers),
];
const SYSTEM_SWITCHES: [(SystemChannelFlags, &str, &str); 3] = [
	(
		SystemChannelFlags::SUPPRESS_JOIN_NOTIFICATIONS,
		"server-overview-joins",
		"server-overview-joins-hint",
	),
	(
		SystemChannelFlags::SUPPRESS_PREMIUM_SUBSCRIPTIONS,
		"server-overview-boosts",
		"server-overview-boosts-hint",
	),
	(
		SystemChannelFlags::SUPPRESS_JOIN_NOTIFICATION_REPLIES,
		"server-overview-stickers",
		"server-overview-stickers-hint",
	),
];

#[derive(Clone, Copy, PartialEq, Default)]
struct Behavior {
	system_channel: Option<Snowflake>,
	system_flags: SystemChannelFlags,
	afk_channel: Option<Snowflake>,
	afk_timeout: u32,
	notifications: DefaultMessageNotifications,
	verification: VerificationLevel,
	content_filter: ExplicitContentFilter,
}

impl Behavior {
	fn of(guild: &GuildState) -> Self {
		Self {
			system_channel: guild.system_channel_id,
			system_flags: guild.system_channel_flags,
			afk_channel: guild.afk_channel_id,
			afk_timeout: guild.afk_timeout,
			notifications: guild
				.default_notifications
				.unwrap_or(DefaultMessageNotifications::AllMessages),
			verification: guild.verification_level.unwrap_or_default(),
			content_filter: guild.explicit_content_filter.unwrap_or_default(),
		}
	}
}

#[derive(Clone, Copy)]
enum Backdrop {
	Banner,
	Splash,
}

impl Backdrop {
	fn id(self) -> &'static str {
		match self {
			Self::Banner => "overview-banner",
			Self::Splash => "overview-splash",
		}
	}

	fn title(self) -> &'static str {
		match self {
			Self::Banner => "server-overview-banner",
			Self::Splash => "server-overview-splash",
		}
	}

	fn upload(self) -> &'static str {
		match self {
			Self::Banner => "server-overview-upload-banner",
			Self::Splash => "server-overview-upload-splash",
		}
	}

	fn description(self) -> &'static str {
		match self {
			Self::Banner => "server-overview-banner-hint",
			Self::Splash => "server-overview-splash-hint",
		}
	}

	fn fit(self) -> Fit {
		let (width, height) = match self {
			Self::Banner => BANNER_SIZE,
			Self::Splash => SPLASH_SIZE,
		};
		Fit::Fill(width, height)
	}
}

#[derive(Default)]
struct Facts {
	name: String,
	icon_url: Option<String>,
	owner: bool,
	banner: bool,
	splash: bool,
	text_channels: Vec<(Snowflake, String)>,
	voice_channels: Vec<(Snowflake, String)>,
}

fn channels_where(guild: &GuildState, wanted: &[ChannelType]) -> Vec<(Snowflake, String)> {
	let mut found: Vec<_> = guild
		.channels
		.values()
		.filter(|channel| wanted.contains(&channel.kind))
		.map(|channel| {
			(
				channel.position.unwrap_or(0),
				channel.id,
				channel.name.clone().unwrap_or_default(),
			)
		})
		.collect();
	found.sort_unstable_by_key(|(position, id, _)| (*position, *id));
	found.into_iter().map(|(_, id, name)| (id, name)).collect()
}

pub struct OverviewPage {
	data: Entity<Data>,
	guild: Snowflake,
	name: Entity<InputState>,
	description: Entity<TextareaState>,
	initial_name: String,
	initial_description: String,
	behavior: Behavior,
	initial_behavior: Behavior,
	icon: Option<Picked>,
	banner: Option<Picked>,
	splash: Option<Picked>,
	facts: Facts,
	_subscriptions: Vec<Subscription>,
}

fn read_facts(data: &Data, guild: Snowflake) -> Facts {
	let state = data.state();
	let Some(found) = state.guild(guild) else {
		return Facts::default();
	};
	let has = |feature: &str| found.features.iter().any(|candidate| candidate == feature);
	Facts {
		name: found.name.clone(),
		icon_url: found
			.icon
			.as_deref()
			.map(|hash| media::cdn::guild_icon_url(guild.get(), hash, 256)),
		owner: found.owner_id.is_some() && found.owner_id == state.current_user_id(),
		banner: has("BANNER"),
		splash: has("INVITE_SPLASH"),
		text_channels: channels_where(found, &[ChannelType::GuildText, ChannelType::GuildAnnouncement]),
		voice_channels: channels_where(found, &[ChannelType::GuildVoice, ChannelType::GuildStageVoice]),
	}
}

impl OverviewPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let (initial_name, initial_description, behavior) = {
			let state = data.read(cx).state();
			state
				.guild(guild)
				.map(|found| {
					(
						found.name.clone(),
						found.description.clone().unwrap_or_default(),
						Behavior::of(found),
					)
				})
				.unwrap_or_default()
		};
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(initial_name.clone(), window, cx);
			state
		});
		let placeholder = cx.t("server-overview-description-placeholder");
		let description = cx.new(|cx| {
			let mut state = TextareaState::new(window, cx)
				.auto_grow(3, 8)
				.placeholder(placeholder);
			state.set_value(initial_description.clone(), window, cx);
			state
		});
		let subscriptions = vec![
			cx.subscribe_in(&name, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
			cx.subscribe_in(&description, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
			cx.subscribe(&data, move |this, data, event: &DataEvent, cx| {
				if let DataEvent::Changed(changes) = event
					&& (changes.guild_list || changes.channel_lists.contains(guild))
				{
					this.facts = read_facts(data.read(cx), guild);
					cx.notify();
				}
			}),
		];
		let facts = read_facts(data.read(cx), guild);
		Self {
			data,
			guild,
			name,
			description,
			initial_name,
			initial_description,
			behavior,
			initial_behavior: behavior,
			icon: None,
			banner: None,
			splash: None,
			facts,
			_subscriptions: subscriptions,
		}
	}

	fn current_name(&self, cx: &App) -> String {
		self.name
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(NAME_LIMIT)
			.collect()
	}

	fn current_description(&self, cx: &App) -> String {
		self.description.read(cx).value().trim().to_owned()
	}

	fn dirty(&self, cx: &App) -> bool {
		self.icon.is_some()
			|| self.banner.is_some()
			|| self.splash.is_some()
			|| self.behavior != self.initial_behavior
			|| (self.current_name(cx) != self.initial_name && !self.current_name(cx).is_empty())
			|| self.current_description(cx) != self.initial_description
	}

	fn save(&mut self, cx: &mut Context<Self>) {
		let name = self.current_name(cx);
		let description = self.current_description(cx);
		let now = self.behavior;
		let was = self.initial_behavior;
		self.data.read(cx).send(Command::EditGuild {
			guild_id: self.guild,
			edit: GuildEdit {
				name: (name != self.initial_name && !name.is_empty()).then(|| name.clone()),
				description: (description != self.initial_description).then(|| description.clone()),
				icon_png: self.icon.take().map(|picked| picked.bytes),
				banner_png: self.banner.take().map(|picked| picked.bytes),
				splash_png: self.splash.take().map(|picked| picked.bytes),
				system_channel_id: (now.system_channel != was.system_channel).then_some(now.system_channel),
				system_channel_flags: (now.system_flags != was.system_flags).then_some(now.system_flags),
				afk_channel_id: (now.afk_channel != was.afk_channel).then_some(now.afk_channel),
				afk_timeout: (now.afk_timeout != was.afk_timeout).then_some(now.afk_timeout),
				default_notifications: (now.notifications != was.notifications).then_some(now.notifications),
				verification_level: (now.verification != was.verification).then_some(now.verification),
				explicit_content_filter: (now.content_filter != was.content_filter)
					.then_some(now.content_filter),
			},
		});
		if !name.is_empty() {
			self.initial_name = name;
		}
		self.initial_description = description;
		self.initial_behavior = now;
		cx.notify();
	}

	fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let name = self.initial_name.clone();
		let description = self.initial_description.clone();
		self.name
			.update(cx, |state, cx| state.set_value(name, window, cx));
		self.description
			.update(cx, |state, cx| state.set_value(description, window, cx));
		self.icon = None;
		self.banner = None;
		self.splash = None;
		self.behavior = self.initial_behavior;
		cx.notify();
	}

	fn identity(&self, facts: &Facts, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let preview: SharedString = facts.name.clone().into();
		let tile = div()
			.size(theme.portrait_lg * 2.)
			.flex_none()
			.rounded(theme.round(theme.portrait_lg * 2.))
			.overflow_hidden()
			.child(match (&self.icon, &facts.icon_url) {
				(Some(picked), _) => img(picked.preview.clone())
					.size_full()
					.object_fit(ObjectFit::Cover)
					.into_any_element(),
				(None, url) => {
					let avatar = Avatar::new(preview).size(theme.portrait_lg * 2.);
					match url {
						Some(url) => avatar.url(url.clone()).into_any_element(),
						None => avatar.into_any_element(),
					}
				}
			});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.child(
				div().flex().items_center().gap(theme.space_24).child(tile).child(
					div()
						.flex()
						.flex_col()
						.items_start()
						.gap(theme.space_8)
						.child(
							Button::new("overview-icon")
								.primary()
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
			.child(field(
				cx.t("server-overview-description"),
				theme,
				div()
					.rounded(theme.radius_sm)
					.bg(theme.input_background)
					.px(theme.space_12)
					.py(theme.space_8)
					.child(
						Textarea::new(&self.description)
							.appearance(false)
							.bordered(false)
							.p_0(),
					),
			))
			.into_any_element()
	}

	fn channel_field(
		&self,
		key: &'static str,
		current: Option<Snowflake>,
		options: &[(Snowflake, String)],
		apply: fn(&mut Behavior, Option<Snowflake>),
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut labels: Vec<SharedString> = vec![cx.t("server-overview-no-channel")];
		labels.extend(options.iter().map(|(_, name)| SharedString::from(name.clone())));
		let mut ids: Vec<Option<Snowflake>> = vec![None];
		ids.extend(options.iter().map(|(id, _)| Some(*id)));
		let selected = ids.iter().position(|id| *id == current).unwrap_or(0);
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			if let Some(choice) = ids.get(index).copied() {
				weak.update(cx, |this, cx| {
					apply(&mut this.behavior, choice);
					cx.notify();
				})
				.ok();
			}
		});
		field(
			cx.t(key),
			theme,
			select(key, labels[selected].clone(), &labels, selected, pick),
		)
	}

	fn system_messages(&self, facts: &Facts, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let switches: Vec<AnyElement> = SYSTEM_SWITCHES
			.iter()
			.map(|(flag, title, description)| {
				let flag = *flag;
				let weak = cx.weak_entity();
				let toggle: ToggleHandler = Rc::new(move |enabled, _, cx| {
					weak.update(cx, |this, cx| {
						this.behavior.system_flags.set(flag, !enabled);
						cx.notify();
					})
					.ok();
				});
				switch_row(
					*title,
					cx.t(title),
					Some(cx.t(description)),
					!self.behavior.system_flags.contains(flag),
					theme,
					toggle,
				)
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(self.section_title("server-overview-system", theme, cx))
			.child(self.channel_field(
				"server-overview-system-channel",
				self.behavior.system_channel,
				&facts.text_channels,
				|behavior, channel| behavior.system_channel = channel,
				theme,
				cx,
			))
			.children(switches)
			.into_any_element()
	}

	fn inactivity(&self, facts: &Facts, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let labels: Vec<SharedString> = AFK_TIMEOUTS.iter().map(|(key, _)| cx.t(key)).collect();
		let selected = AFK_TIMEOUTS
			.iter()
			.position(|(_, seconds)| *seconds == self.behavior.afk_timeout)
			.unwrap_or(1);
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			if let Some((_, seconds)) = AFK_TIMEOUTS.get(index) {
				weak.update(cx, |this, cx| {
					this.behavior.afk_timeout = *seconds;
					cx.notify();
				})
				.ok();
			}
		});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(self.section_title("server-overview-afk", theme, cx))
			.child(self.channel_field(
				"server-overview-afk-channel",
				self.behavior.afk_channel,
				&facts.voice_channels,
				|behavior, channel| behavior.afk_channel = channel,
				theme,
				cx,
			))
			.child(field(
				cx.t("server-overview-afk-timeout"),
				theme,
				select(
					"server-overview-afk-timeout",
					labels[selected].clone(),
					&labels,
					selected,
					pick,
				),
			))
			.into_any_element()
	}

	fn safety(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let levels: Vec<SharedString> = VERIFICATIONS.iter().map(|(key, _)| cx.t(key)).collect();
		let level = VERIFICATIONS
			.iter()
			.position(|(_, level)| *level == self.behavior.verification)
			.unwrap_or(0);
		let filters: Vec<SharedString> = FILTERS.iter().map(|(key, _)| cx.t(key)).collect();
		let filter = FILTERS
			.iter()
			.position(|(_, filter)| *filter == self.behavior.content_filter)
			.unwrap_or(0);
		let weak_level = cx.weak_entity();
		let pick_level: PickHandler = Rc::new(move |index, _, cx| {
			if let Some((_, level)) = VERIFICATIONS.get(index) {
				weak_level
					.update(cx, |this, cx| {
						this.behavior.verification = *level;
						cx.notify();
					})
					.ok();
			}
		});
		let weak_filter = cx.weak_entity();
		let pick_filter: PickHandler = Rc::new(move |index, _, cx| {
			if let Some((_, filter)) = FILTERS.get(index) {
				weak_filter
					.update(cx, |this, cx| {
						this.behavior.content_filter = *filter;
						cx.notify();
					})
					.ok();
			}
		});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(self.section_title("server-overview-safety", theme, cx))
			.child(field(
				cx.t("server-overview-verification"),
				theme,
				select(
					"server-overview-verification",
					levels[level].clone(),
					&levels,
					level,
					pick_level,
				),
			))
			.child(hint(cx.t("server-overview-verification-hint"), theme))
			.child(field(
				cx.t("server-overview-filter"),
				theme,
				select(
					"server-overview-filter",
					filters[filter].clone(),
					&filters,
					filter,
					pick_filter,
				),
			))
			.child(hint(cx.t("server-overview-filter-hint"), theme))
			.into_any_element()
	}

	fn notifications(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let labels: Vec<SharedString> = NOTIFICATIONS.iter().map(|(key, _)| cx.t(key)).collect();
		let selected = NOTIFICATIONS
			.iter()
			.position(|(_, level)| *level == self.behavior.notifications)
			.unwrap_or(0);
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			if let Some((_, level)) = NOTIFICATIONS.get(index) {
				weak.update(cx, |this, cx| {
					this.behavior.notifications = *level;
					cx.notify();
				})
				.ok();
			}
		});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(self.section_title("server-overview-notifications", theme, cx))
			.child(segmented(
				"server-overview-notifications",
				&labels,
				selected,
				theme,
				pick,
			))
			.child(hint(cx.t("server-overview-notifications-hint"), theme))
			.into_any_element()
	}

	fn section_title(&self, key: &'static str, theme: &Theme, cx: &Context<Self>) -> AnyElement {
		div()
			.text_size(theme.font_md)
			.font_weight(gpui::FontWeight::BOLD)
			.text_color(theme.header_primary)
			.child(cx.t(key))
			.into_any_element()
	}

	fn picked(&self, backdrop: Backdrop) -> Option<&Picked> {
		match backdrop {
			Backdrop::Banner => self.banner.as_ref(),
			Backdrop::Splash => self.splash.as_ref(),
		}
	}

	fn set_picked(&mut self, backdrop: Backdrop, picked: Picked) {
		match backdrop {
			Backdrop::Banner => self.banner = Some(picked),
			Backdrop::Splash => self.splash = Some(picked),
		}
	}

	fn image_row(&self, backdrop: Backdrop, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let tile = div()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.w(rem(16.))
			.h(rem(9.))
			.rounded(theme.radius_md)
			.overflow_hidden()
			.bg(theme.input_background)
			.child(match self.picked(backdrop) {
				Some(picked) => img(picked.preview.clone())
					.size_full()
					.object_fit(ObjectFit::Cover)
					.into_any_element(),
				None => Icon::new(IconKind::Image)
					.size(theme.icon_lg)
					.color(theme.interactive_muted)
					.into_any_element(),
			});
		div()
			.flex()
			.items_center()
			.gap(theme.space_24)
			.child(tile)
			.child(
				div()
					.flex()
					.flex_col()
					.items_start()
					.gap(theme.space_8)
					.child(self.section_title(backdrop.title(), theme, cx))
					.child(
						Button::new(backdrop.id())
							.outline()
							.label(cx.t(backdrop.upload()))
							.on_click(cx.listener(move |_, _, _, cx| {
								pick_image(
									cx,
									backdrop.fit(),
									IMAGE_TYPES,
									move |this: &mut Self, picked, cx| {
										this.set_picked(backdrop, picked);
										cx.notify();
									},
								)
							})),
					)
					.child(hint(cx.t(backdrop.description()), theme)),
			)
			.into_any_element()
	}

	fn danger_zone(&self, facts: &Facts, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let guild = self.guild;
		let name = facts.name.clone();
		let transfer_name = facts.name.clone();
		let transfer = div()
			.flex()
			.items_center()
			.justify_between()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.flex_col()
					.child(self.section_title("server-overview-transfer", theme, cx))
					.child(hint(cx.t("server-overview-transfer-hint"), theme)),
			)
			.child(
				Button::new("overview-transfer")
					.danger()
					.outline()
					.label(cx.t("server-overview-transfer"))
					.on_click(move |_, window, cx| transfer::open(window, cx, guild, transfer_name.clone())),
			);
		let delete = div()
			.flex()
			.items_center()
			.justify_between()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.flex_col()
					.child(self.section_title("server-overview-delete", theme, cx))
					.child(hint(cx.t("server-overview-delete-hint"), theme)),
			)
			.child(
				Button::new("overview-delete")
					.danger()
					.outline()
					.label(cx.t("server-overview-delete"))
					.on_click(move |_, window, cx| open_delete(window, cx, guild, name.clone())),
			);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(transfer)
			.child(delete)
			.into_any_element()
	}
}

impl Render for OverviewPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let facts = &self.facts;
		let dirty = self.dirty(cx);
		let weak_reset = cx.weak_entity();
		let weak_save = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-overview"), &theme))
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_32)
					.pb(theme.space_32)
					.child(self.identity(facts, &theme, cx))
					.when(facts.banner, |page| {
						page.child(self.image_row(Backdrop::Banner, &theme, cx))
					})
					.when(facts.splash, |page| {
						page.child(self.image_row(Backdrop::Splash, &theme, cx))
					})
					.child(self.system_messages(facts, &theme, cx))
					.child(self.inactivity(facts, &theme, cx))
					.child(self.notifications(&theme, cx))
					.child(self.safety(&theme, cx))
					.when(facts.owner, |page| {
						page.child(self.danger_zone(facts, &theme, cx))
					})
					.overflow_y_scrollbar()
					.id("overview-scroll")
					.flex_1()
					.min_h_0(),
			)
			.when(dirty, |page| {
				page.child(kit::save_bar(
					&theme,
					cx,
					move |window, cx| {
						weak_reset.update(cx, |this, cx| this.reset(window, cx)).ok();
					},
					move |_, cx| {
						weak_save.update(cx, |this, cx| this.save(cx)).ok();
					},
				))
			})
	}
}

struct DeleteServer {
	data: Entity<Data>,
	guild: Snowflake,
	name: String,
	typed: Entity<InputState>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for DeleteServer {}

fn open_delete(window: &mut Window, cx: &mut App, guild: Snowflake, name: String) {
	let data = data_handle(cx);
	let placeholder = SharedString::from(name.clone());
	let title = cx.t_args(
		"server-delete-title",
		&[("name", crate::i18n::ArgValue::from(name.clone()))],
	);
	let view = cx.new(|cx| {
		let typed = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscription = cx.subscribe_in(&typed, window, |_, _, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		});
		DeleteServer {
			data,
			guild,
			name,
			typed,
			_subscription: subscription,
		}
	});
	let focus = view.read(cx).typed.read(cx).focus_handle(cx);
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl Render for DeleteServer {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let confirmed = self.typed.read(cx).value().trim() == self.name;
		let weak_cancel = cx.weak_entity();
		let weak_delete = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(hint(cx.t("server-delete-body"), &theme))
			.child(field(
				cx.t("server-overview-name"),
				&theme,
				Input::new(&self.typed),
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("server-overview-delete"),
				true,
				confirmed,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_delete
						.update(cx, |this, cx| {
							this.data
								.read(cx)
								.send(Command::DeleteGuild { guild_id: this.guild });
							cx.emit(Close);
						})
						.ok();
				},
			))
	}
}
