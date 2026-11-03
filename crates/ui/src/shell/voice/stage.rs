use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::{ChannelType, Permissions, Snowflake};
use session::Command;
use state::State;

use crate::components::{dialog_footer, field};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::shell::community::access::GuildAccess;
use crate::shell::community::data_handle;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const TOPIC_LIMIT: usize = 120;

#[derive(Clone, Copy)]
pub struct SeatChange {
	pub label: &'static str,
	pub icon: IconKind,
	guild_id: Snowflake,
	channel_id: Snowflake,
	user_id: Snowflake,
	speaker: bool,
}

impl SeatChange {
	pub fn command(self) -> Command {
		Command::SetSpeaker {
			guild_id: self.guild_id,
			channel_id: self.channel_id,
			user_id: self.user_id,
			speaker: self.speaker,
		}
	}
}

pub fn seat_change(state: &State, guild: Snowflake, target: Snowflake) -> Option<SeatChange> {
	let me = state.current_user_id()?;
	let voice = state.guild(guild)?.voice_states.get(&target)?;
	let channel = voice.channel_id?;
	if state.channel(channel)?.kind != ChannelType::GuildStageVoice {
		return None;
	}
	let access = GuildAccess::of(state, guild);
	let speaker = !voice.suppress;
	let own = me == target;
	let can_mute = access.has(Permissions::MUTE_MEMBERS);
	let allowed = if own {
		speaker || can_mute
	} else {
		can_mute && access.outranks(state, guild, target)
	};
	let (label, icon) = match (speaker, own) {
		(true, _) => ("mod-move-audience", IconKind::Members),
		(false, true) => ("voice-stage-speak", IconKind::Stage),
		(false, false) => ("mod-invite-speak", IconKind::Stage),
	};
	allowed.then_some(SeatChange {
		label,
		icon,
		guild_id: guild,
		channel_id: channel,
		user_id: target,
		speaker: !speaker,
	})
}

pub struct Topic {
	data: Entity<Data>,
	channel: Snowflake,
	live: bool,
	input: Entity<InputState>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for Topic {}

pub fn open_topic(window: &mut Window, cx: &mut App, channel: Snowflake, current: Option<String>) {
	let data = data_handle(cx);
	let live = current.is_some();
	let placeholder = cx.t("voice-stage-topic");
	let view = cx.new(|cx| {
		let input = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(placeholder);
			state.set_value(current.unwrap_or_default(), window, cx);
			state
		});
		let subscription = cx.subscribe_in(
			&input,
			window,
			|this: &mut Topic, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::PressEnter { .. }) {
					this.save(cx);
				}
			},
		);
		Topic {
			data,
			channel,
			live,
			input,
			_subscription: subscription,
		}
	});
	let focus = view.read(cx).input.read(cx).focus_handle(cx);
	let title = cx.t(if live {
		"voice-stage-edit"
	} else {
		"voice-stage-start"
	});
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl Topic {
	fn save(&mut self, cx: &mut Context<Self>) {
		let topic: String = self
			.input
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(TOPIC_LIMIT)
			.collect();
		if topic.is_empty() {
			return;
		}
		let channel_id = self.channel;
		self.data.read(cx).send(if self.live {
			Command::EditStageTopic { channel_id, topic }
		} else {
			Command::StartStage { channel_id, topic }
		});
		cx.emit(Close);
	}
}

impl Render for Topic {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let weak_cancel = cx.weak_entity();
		let weak_save = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(cx.t("voice-stage-topic"), &theme, Input::new(&self.input)))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t(if self.live {
					"settings-save"
				} else {
					"voice-stage-start"
				}),
				false,
				true,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_save.update(cx, |this, cx| this.save(cx)).ok();
				},
			))
	}
}
