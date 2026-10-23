use gpui::{
	Context, Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render,
	SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use protocol::Snowflake;
use session::Command;

use crate::components::Avatar;
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::lookup::avatar_for;
use crate::theme::Theme;

const AVATAR_PIXELS: u32 = 96;

struct Incoming {
	channel: Snowflake,
	name: SharedString,
	avatar: String,
}

pub struct CallToast {
	data: Entity<Data>,
	incoming: Vec<Incoming>,
	_subscription: Subscription,
}

impl CallToast {
	pub fn new(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe_in(&data, window, |this, _, event: &DataEvent, _, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.calls || changes.private_channels)
			{
				this.refresh(cx);
			}
		});
		let mut toast = Self {
			data,
			incoming: Vec::new(),
			_subscription: subscription,
		};
		toast.refresh(cx);
		toast
	}

	fn refresh(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let state = data.state();
		let channels: Vec<Snowflake> = state.incoming_calls().map(|call| call.channel_id).collect();
		self.incoming = if channels.is_empty() {
			Vec::new()
		} else {
			let rows = state.dm_rows();
			channels
				.into_iter()
				.filter_map(|channel| {
					let row = rows.iter().find(|row| row.channel_id == channel)?;
					Some(Incoming {
						channel,
						name: row.name.clone().into(),
						avatar: row
							.recipients
							.first()
							.map(|user| avatar_for(&state, *user, AVATAR_PIXELS))
							.unwrap_or_default(),
					})
				})
				.collect()
		};
		drop(state);
		cx.notify();
	}
}

impl Render for CallToast {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		if self.incoming.is_empty() {
			return div().into_any_element();
		}
		let theme = Theme::current(cx);
		let cards: Vec<_> = self
			.incoming
			.iter()
			.map(|call| {
				let channel = call.channel;
				let data = self.data.clone();
				let decline = self.data.clone();
				div()
					.id(("voice-incoming", channel.get()))
					.flex()
					.flex_col()
					.gap(theme.space_12)
					.w(theme.voice_toast_width)
					.p(theme.space_16)
					.rounded(theme.radius_md)
					.border_1()
					.border_color(theme.divider)
					.bg(theme.background_floating)
					.child(
						div()
							.flex()
							.items_center()
							.gap(theme.space_12)
							.child(
								Avatar::new(call.name.clone())
									.size(theme.voice_control)
									.url(call.avatar.clone()),
							)
							.child(
								div()
									.flex()
									.flex_col()
									.min_w_0()
									.child(
										div()
											.truncate()
											.text_size(theme.font_md)
											.font_weight(FontWeight::SEMIBOLD)
											.text_color(theme.header_primary)
											.child(call.name.clone()),
									)
									.child(
										div()
											.text_size(theme.font_sm)
											.text_color(theme.text_muted)
											.child(cx.t("voice-incoming-call")),
									),
							),
					)
					.child(
						div()
							.flex()
							.gap(theme.space_8)
							.child(
								Button::new(("voice-call-decline", channel.get()))
									.danger()
									.label(cx.t("voice-call-decline"))
									.on_click(move |_, _, cx| {
										decline
											.read(cx)
											.send(Command::DeclineCall { channel_id: channel });
									}),
							)
							.child(
								Button::new(("voice-call-accept", channel.get()))
									.success()
									.label(cx.t("voice-call-accept"))
									.on_click(move |_, _, cx| {
										let data = data.read(cx);
										data.send(Command::SelectChannel {
											guild_id: None,
											channel_id: channel,
										});
										data.send(Command::JoinVoice {
											guild_id: None,
											channel_id: channel,
										});
									}),
							),
					)
			})
			.collect();
		div()
			.absolute()
			.top(theme.title_bar_height + theme.top_bar_height + theme.space_8)
			.right(theme.space_16)
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.children(cards)
			.into_any_element()
	}
}
