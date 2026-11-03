use std::collections::HashSet;
use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
	Window, div, prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::switch::Switch;
use protocol::{
	AutoModAction, AutoModActionType, AutoModEventType, AutoModPreset, AutoModRule, AutoModTriggerType,
	ChannelType, LenientVec, Permissions, Snowflake,
};
use session::Command;
use state::Changes;

use super::kit;
use crate::components::{
	IconButton, PickHandler, ToggleHandler, badge, caption, field, hint, segmented, select, switch_row,
};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::community::access::GuildAccess;
use crate::theme::Theme;

const SAVE_ACTION: &str = "save automod rule";
const NAME_LIMIT: usize = 100;
const TERM_LIMIT: usize = 60;
const PATTERN_LIMIT: usize = 260;
const MESSAGE_LIMIT: usize = 150;
const KEYWORD_COUNT: usize = 1000;
const PATTERN_COUNT: usize = 10;
const ALLOW_COUNT: usize = 100;
const PRESET_ALLOW_COUNT: usize = 1000;
const MENTION_MAX: i64 = 50;
const EXEMPT_ROLE_MAX: usize = 20;
const EXEMPT_CHANNEL_MAX: usize = 50;
const DURATIONS: [i64; 6] = [60, 300, 600, 3600, 86_400, 604_800];
const DEFAULT_DURATION: i64 = 3600;
const DEFAULT_MENTION_LIMIT: i64 = 10;
const NEW_TRIGGERS: [AutoModTriggerType; 5] = [
	AutoModTriggerType::Keyword,
	AutoModTriggerType::Spam,
	AutoModTriggerType::KeywordPreset,
	AutoModTriggerType::MentionSpam,
	AutoModTriggerType::MemberProfile,
];
const PRESETS: [AutoModPreset; 3] = [
	AutoModPreset::Profanity,
	AutoModPreset::SexualContent,
	AutoModPreset::Slurs,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Exempt {
	Roles,
	Channels,
}

impl Exempt {
	fn ids(self, rule: &mut AutoModRule) -> &mut LenientVec<Snowflake> {
		match self {
			Self::Roles => &mut rule.exempt_roles,
			Self::Channels => &mut rule.exempt_channels,
		}
	}

	fn limit(self) -> usize {
		match self {
			Self::Roles => EXEMPT_ROLE_MAX,
			Self::Channels => EXEMPT_CHANNEL_MAX,
		}
	}

	fn title(self) -> &'static str {
		match self {
			Self::Roles => "automod-exempt-roles",
			Self::Channels => "automod-exempt-channels",
		}
	}

	fn add(self) -> &'static str {
		match self {
			Self::Roles => "automod-add-role",
			Self::Channels => "automod-add-channel",
		}
	}

	fn key(self) -> &'static str {
		match self {
			Self::Roles => "automod-exempt-role",
			Self::Channels => "automod-exempt-channel",
		}
	}
}

type EditHandler = Rc<dyn Fn(&mut App)>;

struct Draft {
	original: AutoModRule,
	rule: AutoModRule,
}

struct Saving {
	known: Vec<Snowflake>,
	name: String,
}

struct RuleEntry {
	id: Snowflake,
	name: SharedString,
	trigger: AutoModTriggerType,
	enabled: bool,
}

pub struct AutomodPage {
	data: Entity<Data>,
	guild: Snowflake,
	draft: Option<Draft>,
	dirty: bool,
	saving: Option<Saving>,
	name: Entity<InputState>,
	keywords: Entity<TextareaState>,
	patterns: Entity<TextareaState>,
	allowed: Entity<TextareaState>,
	limit: Entity<InputState>,
	message: Entity<InputState>,
	_subscriptions: Vec<Subscription>,
}

fn trigger_key(trigger: AutoModTriggerType) -> String {
	match i64::from(trigger) {
		value @ (1 | 3 | 4 | 5 | 6) => format!("automod-trigger-{value}"),
		_ => "automod-trigger-unknown".to_owned(),
	}
}

fn max_rules(trigger: AutoModTriggerType) -> usize {
	match trigger {
		AutoModTriggerType::Keyword => 6,
		_ => 1,
	}
}

fn allowed_actions(trigger: AutoModTriggerType) -> &'static [AutoModActionType] {
	match trigger {
		AutoModTriggerType::Keyword | AutoModTriggerType::MentionSpam => &[
			AutoModActionType::BlockMessage,
			AutoModActionType::SendAlert,
			AutoModActionType::Timeout,
		],
		AutoModTriggerType::Spam | AutoModTriggerType::KeywordPreset => {
			&[AutoModActionType::BlockMessage, AutoModActionType::SendAlert]
		}
		AutoModTriggerType::MemberProfile => &[
			AutoModActionType::SendAlert,
			AutoModActionType::BlockMemberInteraction,
		],
		AutoModTriggerType::Unknown(_) => &[],
	}
}

fn has_terms(trigger: AutoModTriggerType) -> bool {
	matches!(
		trigger,
		AutoModTriggerType::Keyword | AutoModTriggerType::MemberProfile
	)
}

fn lines(text: &str, length: usize, count: usize) -> Vec<String> {
	let mut found: Vec<String> = Vec::new();
	let mut seen: HashSet<String> = HashSet::new();
	for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
		let term: String = line.chars().take(length).collect();
		if seen.insert(term.clone()) {
			found.push(term);
		}
		if found.len() == count {
			break;
		}
	}
	found
}

fn new_action(kind: AutoModActionType) -> AutoModAction {
	let mut action = AutoModAction {
		kind,
		..AutoModAction::default()
	};
	if kind == AutoModActionType::Timeout {
		action.metadata.duration_seconds = Some(DEFAULT_DURATION);
	}
	action
}

fn new_rule(trigger: AutoModTriggerType) -> AutoModRule {
	let member = trigger == AutoModTriggerType::MemberProfile;
	let mut rule = AutoModRule {
		event_type: if member {
			AutoModEventType::MemberUpdate
		} else {
			AutoModEventType::MessageSend
		},
		trigger_type: trigger,
		enabled: true,
		actions: LenientVec::from(vec![new_action(if member {
			AutoModActionType::BlockMemberInteraction
		} else {
			AutoModActionType::BlockMessage
		})]),
		..AutoModRule::default()
	};
	match trigger {
		AutoModTriggerType::KeywordPreset => rule.trigger_metadata.presets.push(AutoModPreset::Profanity),
		AutoModTriggerType::MentionSpam => {
			rule.trigger_metadata.mention_total_limit = Some(DEFAULT_MENTION_LIMIT);
		}
		_ => {}
	}
	rule
}

fn text_box(state: &Entity<TextareaState>, theme: &Theme) -> impl IntoElement {
	div()
		.rounded(theme.radius_sm)
		.bg(theme.input_background)
		.px(theme.space_12)
		.py(theme.space_8)
		.child(Textarea::new(state).appearance(false).bordered(false).p_0())
}

impl AutomodPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		data.read(cx).send(Command::LoadAutomodRules { guild_id: guild });
		let name = cx.new(|cx| InputState::new(window, cx));
		let limit = cx.new(|cx| InputState::new(window, cx));
		let message_placeholder = cx.t("automod-message-placeholder");
		let message = cx.new(|cx| InputState::new(window, cx).placeholder(message_placeholder));
		let area = |window: &mut Window, cx: &mut Context<Self>| {
			cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 8))
		};
		let keywords = area(window, cx);
		let patterns = area(window, cx);
		let allowed = area(window, cx);
		let mut subscriptions = Vec::new();
		for input in [&name, &limit, &message] {
			subscriptions.push(
				cx.subscribe_in(input, window, |this, _, event: &InputEvent, _, cx| {
					if matches!(event, InputEvent::Change) {
						this.refresh_dirty(cx);
						cx.notify();
					}
				}),
			);
		}
		for input in [&keywords, &patterns, &allowed] {
			subscriptions.push(
				cx.subscribe_in(input, window, |this, _, event: &InputEvent, _, cx| {
					if matches!(event, InputEvent::Change) {
						this.refresh_dirty(cx);
						cx.notify();
					}
				}),
			);
		}
		subscriptions.push(
			cx.subscribe_in(&data, window, |this, _, event: &DataEvent, window, cx| {
				if let DataEvent::Changed(changes) = event {
					this.on_changes(changes, window, cx);
				}
			}),
		);
		Self {
			data,
			guild,
			draft: None,
			dirty: false,
			saving: None,
			name,
			keywords,
			patterns,
			allowed,
			limit,
			message,
			_subscriptions: subscriptions,
		}
	}

	fn begin(&mut self, rule: AutoModRule, window: &mut Window, cx: &mut Context<Self>) {
		let metadata = &rule.trigger_metadata;
		let message = rule
			.action(AutoModActionType::BlockMessage)
			.and_then(|action| action.metadata.custom_message.clone())
			.unwrap_or_default();
		let limit = metadata
			.mention_total_limit
			.map(|limit| limit.to_string())
			.unwrap_or_default();
		self.name
			.update(cx, |state, cx| state.set_value(rule.name.clone(), window, cx));
		self.keywords.update(cx, |state, cx| {
			state.set_value(metadata.keyword_filter.join("\n"), window, cx)
		});
		self.patterns.update(cx, |state, cx| {
			state.set_value(metadata.regex_patterns.join("\n"), window, cx)
		});
		self.allowed.update(cx, |state, cx| {
			state.set_value(metadata.allow_list.join("\n"), window, cx)
		});
		self.limit
			.update(cx, |state, cx| state.set_value(limit, window, cx));
		self.message
			.update(cx, |state, cx| state.set_value(message, window, cx));
		self.draft = Some(Draft {
			original: rule.clone(),
			rule,
		});
		self.refresh_dirty(cx);
		cx.notify();
	}

	fn refresh_dirty(&mut self, cx: &App) {
		self.dirty = self.draft.as_ref().is_some_and(|draft| {
			let assembled = self.assembled(&draft.rule, cx);
			assembled != draft.original && Self::valid(&assembled)
		});
	}

	fn on_changes(&mut self, changes: &Changes, window: &mut Window, cx: &mut Context<Self>) {
		let Some(saving) = &self.saving else {
			return;
		};
		let rejected = changes.rejected
			&& self
				.data
				.read(cx)
				.state()
				.last_rejection()
				.is_some_and(|rejection| rejection.reason.starts_with(SAVE_ACTION));
		if rejected {
			self.saving = None;
			cx.notify();
			return;
		}
		let Some(draft) = &self.draft else {
			return;
		};
		if !changes.loaded.contains(self.guild) {
			return;
		}
		let saved = self
			.data
			.read(cx)
			.state()
			.automod_rules(self.guild)
			.iter()
			.find(|rule| {
				if draft.rule.id == Snowflake::default() {
					!saving.known.contains(&rule.id)
						&& rule.name == saving.name
						&& rule.trigger_type == draft.rule.trigger_type
				} else {
					rule.id == draft.rule.id && **rule != draft.original
				}
			})
			.cloned();
		if let Some(rule) = saved {
			self.saving = None;
			self.begin(rule, window, cx);
		}
	}

	fn create(&mut self, trigger: AutoModTriggerType, window: &mut Window, cx: &mut Context<Self>) {
		self.begin(new_rule(trigger), window, cx);
	}

	fn retrigger(&mut self, trigger: AutoModTriggerType, window: &mut Window, cx: &mut Context<Self>) {
		let name = self.name.read(cx).value().to_string();
		let mut rule = new_rule(trigger);
		rule.name = name;
		self.begin(rule, window, cx);
	}

	fn assembled(&self, rule: &AutoModRule, cx: &App) -> AutoModRule {
		let mut rule = rule.clone();
		rule.name = self
			.name
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(NAME_LIMIT)
			.collect();
		let metadata = &mut rule.trigger_metadata;
		match rule.trigger_type {
			AutoModTriggerType::Keyword | AutoModTriggerType::MemberProfile => {
				metadata.keyword_filter = lines(&self.keywords.read(cx).value(), TERM_LIMIT, KEYWORD_COUNT);
				metadata.regex_patterns =
					lines(&self.patterns.read(cx).value(), PATTERN_LIMIT, PATTERN_COUNT);
				metadata.allow_list = lines(&self.allowed.read(cx).value(), TERM_LIMIT, ALLOW_COUNT);
			}
			AutoModTriggerType::KeywordPreset => {
				metadata.allow_list = lines(&self.allowed.read(cx).value(), TERM_LIMIT, PRESET_ALLOW_COUNT);
			}
			AutoModTriggerType::MentionSpam => {
				metadata.mention_total_limit = self
					.limit
					.read(cx)
					.value()
					.trim()
					.parse::<i64>()
					.ok()
					.map(|limit| limit.clamp(1, MENTION_MAX));
			}
			_ => {}
		}
		let message = self.message.read(cx).value();
		let message = message.trim();
		if let Some(block) = rule
			.actions
			.iter_mut()
			.find(|action| action.kind == AutoModActionType::BlockMessage)
		{
			block.metadata.custom_message =
				(!message.is_empty()).then(|| message.chars().take(MESSAGE_LIMIT).collect());
		}
		rule
	}

	fn valid(rule: &AutoModRule) -> bool {
		let metadata = &rule.trigger_metadata;
		let configured = match rule.trigger_type {
			AutoModTriggerType::Keyword | AutoModTriggerType::MemberProfile => {
				!metadata.keyword_filter.is_empty() || !metadata.regex_patterns.is_empty()
			}
			AutoModTriggerType::KeywordPreset => !metadata.presets.is_empty(),
			AutoModTriggerType::MentionSpam => metadata.mention_total_limit.is_some(),
			_ => true,
		};
		let alert_ready = rule
			.action(AutoModActionType::SendAlert)
			.is_none_or(|alert| alert.metadata.channel_id.is_some());
		!rule.name.is_empty() && !rule.actions.is_empty() && configured && alert_ready
	}

	fn save(&mut self, cx: &mut Context<Self>) {
		let Some(draft) = &self.draft else {
			return;
		};
		let rule = self.assembled(&draft.rule, cx);
		let known = self
			.data
			.read(cx)
			.state()
			.automod_rules(self.guild)
			.iter()
			.map(|known| known.id)
			.collect();
		let original = (rule.id != Snowflake::default()).then(|| Box::new(draft.original.clone()));
		self.saving = Some(Saving {
			known,
			name: rule.name.clone(),
		});
		self.data.read(cx).send(Command::SaveAutomodRule {
			guild_id: self.guild,
			rule: Box::new(rule),
			original,
		});
		cx.notify();
	}

	fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(rule) = self.draft.as_ref().map(|draft| draft.original.clone()) {
			self.begin(rule, window, cx);
		}
	}

	fn set_enabled(&self, rule_id: Snowflake, enabled: bool, cx: &App) {
		self.data.read(cx).send(Command::SetAutomodEnabled {
			guild_id: self.guild,
			rule_id,
			enabled,
		});
	}

	fn open_rule(&mut self, rule_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		let rule = self
			.data
			.read(cx)
			.state()
			.automod_rules(self.guild)
			.iter()
			.find(|rule| rule.id == rule_id)
			.cloned();
		if let Some(rule) = rule {
			self.begin(rule, window, cx);
		}
	}

	fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(rule_id) = self.draft.as_ref().map(|draft| draft.rule.id) else {
			return;
		};
		let data = self.data.clone();
		let guild = self.guild;
		let weak = cx.weak_entity();
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			let weak = weak.clone();
			alert
				.title(cx.t("automod-delete-title"))
				.description(cx.t("automod-delete-body"))
				.confirm()
				.ok_text(cx.t("automod-delete"))
				.ok_variant(ButtonVariant::Danger)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::DeleteAutomodRule {
						guild_id: guild,
						rule_id,
					});
					weak.update(cx, |this, cx| {
						this.draft = None;
						cx.notify();
					})
					.ok();
					true
				})
		});
	}

	fn edit_handler(
		&self,
		cx: &mut Context<Self>,
		apply: impl Fn(&mut AutoModRule) + 'static,
	) -> EditHandler {
		let weak = cx.weak_entity();
		Rc::new(move |cx| {
			weak.update(cx, |this, cx| {
				if let Some(draft) = &mut this.draft {
					apply(&mut draft.rule);
				}
				this.refresh_dirty(cx);
				cx.notify();
			})
			.ok();
		})
	}

	fn toggle_handler(
		&self,
		cx: &mut Context<Self>,
		apply: impl Fn(&mut AutoModRule, bool) + 'static,
	) -> ToggleHandler {
		let weak = cx.weak_entity();
		Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| {
				if let Some(draft) = &mut this.draft {
					apply(&mut draft.rule, value);
				}
				this.refresh_dirty(cx);
				cx.notify();
			})
			.ok();
		})
	}

	fn pick_handler(
		&self,
		cx: &mut Context<Self>,
		apply: impl Fn(&mut AutoModRule, usize) + 'static,
	) -> PickHandler {
		let weak = cx.weak_entity();
		Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				if let Some(draft) = &mut this.draft {
					apply(&mut draft.rule, index);
				}
				this.refresh_dirty(cx);
				cx.notify();
			})
			.ok();
		})
	}

	fn named_channels(&self, kinds: &[ChannelType], cx: &App) -> Vec<(Snowflake, SharedString)> {
		let state = self.data.read(cx).state();
		let Some(guild) = state.guild(self.guild) else {
			return Vec::new();
		};
		let mut channels: Vec<_> = guild
			.channels
			.values()
			.filter(|channel| kinds.is_empty() || kinds.contains(&channel.kind))
			.collect();
		channels.sort_by_key(|channel| (channel.position, channel.id));
		channels
			.into_iter()
			.map(|channel| {
				let name = channel.name.clone().unwrap_or_default();
				let label = if channel.kind == ChannelType::GuildCategory {
					name
				} else {
					format!("#{name}")
				};
				(channel.id, label.into())
			})
			.collect()
	}

	fn named_roles(&self, cx: &App) -> Vec<(Snowflake, SharedString)> {
		let state = self.data.read(cx).state();
		let Some(guild) = state.guild(self.guild) else {
			return Vec::new();
		};
		let mut roles: Vec<_> = guild
			.roles
			.values()
			.filter(|role| role.id != self.guild)
			.collect();
		roles.sort_by_key(|role| std::cmp::Reverse((role.position, role.id)));
		roles
			.into_iter()
			.map(|role| (role.id, role.name.clone().into()))
			.collect()
	}

	fn trigger_section(&self, rule: &AutoModRule, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let mut section = div().flex().flex_col().gap(theme.space_24);
		if has_terms(rule.trigger_type) {
			section = section
				.child(field(
					cx.t("automod-keywords"),
					theme,
					text_box(&self.keywords, theme),
				))
				.child(hint(cx.t("automod-keywords-hint"), theme))
				.child(field(
					cx.t("automod-patterns"),
					theme,
					text_box(&self.patterns, theme),
				))
				.child(hint(cx.t("automod-patterns-hint"), theme));
		}
		if rule.trigger_type == AutoModTriggerType::KeywordPreset {
			let rows: Vec<AnyElement> = PRESETS
				.iter()
				.map(|preset| {
					let preset = *preset;
					let handler = self.toggle_handler(cx, move |rule, on| {
						let presets = &mut rule.trigger_metadata.presets;
						presets.retain(|known| *known != preset);
						if on {
							presets.push(preset);
						}
					});
					switch_row(
						("automod-preset", i64::from(preset) as usize),
						cx.t(&format!("automod-preset-{}", i64::from(preset))),
						None,
						rule.trigger_metadata.presets.contains(&preset),
						theme,
						handler,
					)
				})
				.collect();
			section = section.child(field(
				cx.t("automod-presets"),
				theme,
				div().flex().flex_col().gap(theme.space_12).children(rows),
			));
		}
		if has_terms(rule.trigger_type) || rule.trigger_type == AutoModTriggerType::KeywordPreset {
			section = section
				.child(field(
					cx.t("automod-allowed"),
					theme,
					text_box(&self.allowed, theme),
				))
				.child(hint(cx.t("automod-allowed-hint"), theme));
		}
		if rule.trigger_type == AutoModTriggerType::MentionSpam {
			let raid = self.toggle_handler(cx, |rule, on| {
				rule.trigger_metadata.mention_raid_protection_enabled = on;
			});
			section = section
				.child(field(
					cx.t("automod-mention-limit"),
					theme,
					div().w(theme.settings_nav_width).child(Input::new(&self.limit)),
				))
				.child(hint(cx.t("automod-mention-limit-hint"), theme))
				.child(switch_row(
					"automod-raid",
					cx.t("automod-raid"),
					Some(cx.t("automod-raid-hint")),
					rule.trigger_metadata.mention_raid_protection_enabled,
					theme,
					raid,
				));
		}
		section.into_any_element()
	}

	fn action_detail(
		&self,
		rule: &AutoModRule,
		kind: AutoModActionType,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let action = rule.action(kind)?;
		match kind {
			AutoModActionType::BlockMessage => {
				Some(field(cx.t("automod-message"), theme, Input::new(&self.message)))
			}
			AutoModActionType::SendAlert => {
				let channels =
					self.named_channels(&[ChannelType::GuildText, ChannelType::GuildAnnouncement], cx);
				let current = action.metadata.channel_id;
				let selected = channels.iter().position(|(id, _)| Some(*id) == current);
				let labels: Vec<SharedString> = channels.iter().map(|(_, label)| label.clone()).collect();
				let ids: Vec<Snowflake> = channels.iter().map(|(id, _)| *id).collect();
				let pick = self.pick_handler(cx, move |rule, index| {
					if let Some(alert) = rule
						.actions
						.iter_mut()
						.find(|action| action.kind == AutoModActionType::SendAlert)
					{
						alert.metadata.channel_id = ids.get(index).copied();
					}
				});
				let label =
					selected.map_or_else(|| cx.t("automod-pick-channel"), |index| labels[index].clone());
				Some(field(
					cx.t("automod-alert-channel"),
					theme,
					div().flex().child(select(
						"automod-alert-channel",
						label,
						&labels,
						selected.unwrap_or(usize::MAX),
						pick,
					)),
				))
			}
			AutoModActionType::Timeout => {
				let seconds = action.metadata.duration_seconds;
				let labels: Vec<SharedString> = DURATIONS
					.iter()
					.map(|value| cx.t(&format!("automod-duration-{value}")))
					.collect();
				let pick = self.pick_handler(cx, |rule, index| {
					if let Some(timeout) = rule
						.actions
						.iter_mut()
						.find(|action| action.kind == AutoModActionType::Timeout)
					{
						timeout.metadata.duration_seconds = DURATIONS.get(index).copied();
					}
				});
				Some(field(
					cx.t("automod-duration"),
					theme,
					segmented(
						"automod-duration",
						&labels,
						DURATIONS
							.iter()
							.position(|value| Some(*value) == seconds)
							.unwrap_or(usize::MAX),
						theme,
						pick,
					),
				))
			}
			_ => None,
		}
	}

	fn actions_section(&self, rule: &AutoModRule, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let can_timeout =
			GuildAccess::of(&self.data.read(cx).state(), self.guild).has(Permissions::MODERATE_MEMBERS);
		let rows: Vec<AnyElement> = allowed_actions(rule.trigger_type)
			.iter()
			.filter(|kind| {
				**kind != AutoModActionType::Timeout || can_timeout || rule.action(**kind).is_some()
			})
			.map(|kind| {
				let kind = *kind;
				let number = i64::from(kind);
				let handler = self.toggle_handler(cx, move |rule, on| {
					rule.actions.retain(|action| action.kind != kind);
					if on {
						rule.actions.push(new_action(kind));
					}
				});
				div()
					.flex()
					.flex_col()
					.gap(theme.space_12)
					.child(switch_row(
						("automod-action", number as usize),
						cx.t(&format!("automod-action-{number}")),
						Some(cx.t(&format!("automod-action-{number}-hint"))),
						rule.action(kind).is_some(),
						theme,
						handler,
					))
					.children(self.action_detail(rule, kind, theme, cx))
					.into_any_element()
			})
			.collect();
		field(
			cx.t("automod-actions"),
			theme,
			div().flex().flex_col().gap(theme.space_16).children(rows),
		)
	}

	fn exempt_field(
		&self,
		kind: Exempt,
		rule: &AutoModRule,
		options: Vec<(Snowflake, SharedString)>,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let current: Vec<Snowflake> = match kind {
			Exempt::Roles => rule.exempt_roles.to_vec(),
			Exempt::Channels => rule.exempt_channels.to_vec(),
		};
		let chips: Vec<AnyElement> = current
			.iter()
			.map(|id| {
				let id = *id;
				let label = options
					.iter()
					.find(|(known, _)| *known == id)
					.map_or_else(|| id.to_string().into(), |(_, label)| label.clone());
				let remove = self.edit_handler(cx, move |rule| kind.ids(rule).retain(|known| *known != id));
				div()
					.flex()
					.items_center()
					.gap(theme.space_4)
					.h(theme.control_height)
					.pl(theme.space_8)
					.rounded(theme.radius_sm)
					.bg(theme.background_tertiary)
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.child(label)
					.child(
						IconButton::new((kind.key(), id.get()), IconKind::Close)
							.icon_size(theme.glyph_sm)
							.on_click(move |_, _, cx| remove(cx)),
					)
					.into_any_element()
			})
			.collect();
		let free: Vec<(Snowflake, SharedString)> = options
			.into_iter()
			.filter(|(id, _)| !current.contains(id))
			.collect();
		let labels: Vec<SharedString> = free.iter().map(|(_, label)| label.clone()).collect();
		let ids: Vec<Snowflake> = free.iter().map(|(id, _)| *id).collect();
		let pick = self.pick_handler(cx, move |rule, index| {
			if let Some(id) = ids.get(index) {
				kind.ids(rule).push(*id);
			}
		});
		let full = current.len() >= kind.limit() || labels.is_empty();
		field(
			cx.t(kind.title()),
			theme,
			div()
				.flex()
				.flex_wrap()
				.items_center()
				.gap(theme.space_8)
				.children(chips)
				.when(!full, |row| {
					row.child(select(kind.key(), cx.t(kind.add()), &labels, usize::MAX, pick))
				}),
		)
	}

	fn trigger_field(&self, rule: &AutoModRule, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let created = rule.id != Snowflake::default();
		if created {
			return field(
				cx.t("automod-trigger"),
				theme,
				badge(cx.t(&trigger_key(rule.trigger_type)), theme.brand, theme),
			);
		}
		let taken = |trigger: AutoModTriggerType| {
			self.data
				.read(cx)
				.state()
				.automod_rules(self.guild)
				.iter()
				.filter(|existing| existing.trigger_type == trigger)
				.count()
		};
		let triggers: Vec<AutoModTriggerType> = NEW_TRIGGERS
			.iter()
			.copied()
			.filter(|trigger| *trigger == rule.trigger_type || taken(*trigger) < max_rules(*trigger))
			.collect();
		let labels: Vec<SharedString> = triggers
			.iter()
			.map(|trigger| cx.t(&trigger_key(*trigger)))
			.collect();
		let selected = triggers
			.iter()
			.position(|trigger| *trigger == rule.trigger_type)
			.unwrap_or(0);
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, window, cx| {
			if let Some(trigger) = triggers.get(index).copied() {
				weak.update(cx, |this, cx| this.retrigger(trigger, window, cx))
					.ok();
			}
		});
		field(
			cx.t("automod-trigger"),
			theme,
			segmented("automod-trigger", &labels, selected, theme, pick),
		)
	}

	fn editor(&self, rule: &AutoModRule, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let dirty = self.dirty && self.saving.is_none();
		let weak_reset = cx.weak_entity();
		let weak_save = cx.weak_entity();
		let created = rule.id != Snowflake::default();
		let enabled = self.toggle_handler(cx, |rule, on| rule.enabled = on);
		let title: SharedString = if rule.name.is_empty() {
			cx.t("server-settings-automod")
		} else {
			rule.name.clone().into()
		};
		let message_trigger = rule.event_type == AutoModEventType::MessageSend;
		let mut body = div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.child(field(cx.t("automod-name"), theme, Input::new(&self.name)))
			.child(self.trigger_field(rule, theme, cx))
			.child(self.trigger_section(rule, theme, cx))
			.child(self.actions_section(rule, theme, cx))
			.child(switch_row(
				"automod-enabled",
				cx.t("automod-enabled"),
				None,
				rule.enabled,
				theme,
				enabled,
			))
			.child(self.exempt_field(Exempt::Roles, rule, self.named_roles(cx), theme, cx));
		if message_trigger {
			body = body.child(self.exempt_field(
				Exempt::Channels,
				rule,
				self.named_channels(&[], cx),
				theme,
				cx,
			));
		}
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_12)
					.pb(theme.space_20)
					.child(
						IconButton::new("automod-back", IconKind::ChevronLeft)
							.tooltip(cx.t("automod-back"))
							.on_click(cx.listener(|this, _, _, cx| {
								this.draft = None;
								this.saving = None;
								cx.notify();
							})),
					)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.text_size(theme.font_xl)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_primary)
							.child(title),
					)
					.when(created, |row| {
						row.child(
							Button::new("automod-delete")
								.danger()
								.outline()
								.label(cx.t("automod-delete"))
								.on_click(cx.listener(|this, _, window, cx| this.confirm_delete(window, cx))),
						)
					}),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.pb(theme.space_32)
					.child(body)
					.overflow_y_scrollbar()
					.id("automod-editor")
					.flex_1()
					.min_h_0(),
			)
			.when(dirty, |editor| {
				editor.child(kit::save_bar(
					theme,
					cx,
					move |window, cx| {
						weak_reset.update(cx, |this, cx| this.reset(window, cx)).ok();
					},
					move |_, cx| {
						weak_save.update(cx, |this, cx| this.save(cx)).ok();
					},
				))
			})
			.into_any_element()
	}

	fn row(&self, rule: &RuleEntry, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let rule_id = rule.id;
		let id = rule_id.get();
		let weak = cx.weak_entity();
		div()
			.flex()
			.items_center()
			.gap(theme.space_12)
			.pr(theme.space_12)
			.h(theme.list_row_height + theme.space_8)
			.rounded(theme.radius_sm)
			.hover(|style| style.bg(theme.modifier_hover))
			.child(
				div()
					.id(("automod-rule", id))
					.flex()
					.flex_1()
					.min_w_0()
					.items_center()
					.gap(theme.space_12)
					.h_full()
					.px(theme.space_12)
					.cursor_pointer()
					.on_click(cx.listener(move |this, _, window, cx| this.open_rule(rule_id, window, cx)))
					.child(
						Icon::new(IconKind::Gavel)
							.size(theme.glyph_md)
							.color(theme.interactive_normal),
					)
					.child(
						div()
							.flex()
							.flex_col()
							.flex_1()
							.min_w_0()
							.child(
								div()
									.truncate()
									.text_size(theme.font_md)
									.font_weight(FontWeight::SEMIBOLD)
									.text_color(theme.header_primary)
									.child(rule.name.clone()),
							)
							.child(
								div()
									.truncate()
									.text_size(theme.font_sm)
									.text_color(theme.text_muted)
									.child(cx.t(&trigger_key(rule.trigger))),
							),
					),
			)
			.child(
				Switch::new(("automod-enabled-row", id))
					.checked(rule.enabled)
					.on_click(move |value: &bool, _, cx| {
						let value = *value;
						weak.update(cx, |this, cx| this.set_enabled(rule_id, value, cx))
							.ok();
					}),
			)
			.into_any_element()
	}

	fn list(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let rules: Vec<RuleEntry> = self
			.data
			.read(cx)
			.state()
			.automod_rules(self.guild)
			.iter()
			.map(|rule| RuleEntry {
				id: rule.id,
				name: rule.name.clone().into(),
				trigger: rule.trigger_type,
				enabled: rule.enabled,
			})
			.collect();
		let rows: Vec<AnyElement> = rules.iter().map(|rule| self.row(rule, theme, cx)).collect();
		let available: Vec<AutoModTriggerType> = NEW_TRIGGERS
			.iter()
			.copied()
			.filter(|trigger| {
				rules.iter().filter(|rule| rule.trigger == *trigger).count() < max_rules(*trigger)
			})
			.collect();
		let empty = rows.is_empty();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-automod"), theme))
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.justify_between()
					.pb(theme.space_16)
					.child(caption(
						cx.t_args("automod-count", &[("count", ArgValue::from(rules.len()))]),
						theme,
					))
					.when(!available.is_empty(), |bar| {
						bar.child(
							Button::new("automod-create")
								.primary()
								.icon(gpui_component::Icon::default().path(IconKind::Plus.path()))
								.label(cx.t("automod-create"))
								.on_click(cx.listener(move |this, _, window, cx| {
									this.create(available[0], window, cx)
								})),
						)
					}),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_2)
					.pb(theme.space_32)
					.children(rows)
					.when(empty, |list| {
						list.child(kit::empty(IconKind::Gavel, cx.t("automod-empty"), theme))
					})
					.overflow_y_scrollbar()
					.id("automod-list")
					.flex_1()
					.min_h_0(),
			)
			.into_any_element()
	}
}

impl Render for AutomodPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		match self.draft.take() {
			Some(draft) => {
				let editor = self.editor(&draft.rule, &theme, cx);
				self.draft = Some(draft);
				editor
			}
			None => self.list(&theme, cx),
		}
	}
}
