use gpui::{
	Context, Entity, InteractiveElement as _, IntoElement, MouseButton, ObjectFit, ParentElement as _,
	Render, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Task, Window, div,
	img, prelude::FluentBuilder as _,
};
use protocol::{ChannelType, Snowflake};
use session::Command;

use super::popout;
use super::video_sink::{follow_frames, video_image};
use crate::components::IconButton;
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::theme::Theme;

const PIP_ASPECT: f32 = 9. / 16.;

struct Target {
	owner: Snowflake,
	stream_key: String,
	guild_id: Option<Snowflake>,
	channel_id: Snowflake,
}

pub struct StreamPip {
	data: Entity<Data>,
	visible: bool,
	_frames: Task<()>,
	_data: Subscription,
}

impl StreamPip {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let observed = cx.observe(&data, |_, _, cx| cx.notify());
		Self {
			data,
			visible: false,
			_frames: follow_frames(cx, |pip, cx| {
				if pip.visible {
					cx.notify();
				}
			}),
			_data: observed,
		}
	}

	fn target(&self, cx: &Context<Self>) -> Option<Target> {
		let data = self.data.read(cx);
		let state = data.state();
		let voice = state.voice()?;
		let stream_key = voice.watching.clone()?;
		let owner = state
			.streams()
			.find(|stream| stream.stream_key == stream_key)?
			.user_id;
		let in_room = state.selected().is_some_and(|selected| {
			selected.channel_id == voice.channel_id
				|| state.channel(selected.channel_id).is_some_and(|channel| {
					matches!(
						channel.kind,
						ChannelType::GuildVoice | ChannelType::GuildStageVoice
					)
				})
		});
		(!in_room).then_some(Target {
			owner,
			stream_key,
			guild_id: voice.guild_id,
			channel_id: voice.channel_id,
		})
	}
}

impl Render for StreamPip {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let target = self.target(cx);
		self.visible = target.is_some();
		let Some(target) = target else {
			return div().into_any_element();
		};
		let theme = Theme::current(cx);
		let image = video_image(cx, target.owner);
		let width = theme.voice_pip_width;
		let (guild_id, channel_id, owner) = (target.guild_id, target.channel_id, target.owner);
		let stream_key = target.stream_key;
		div()
			.id("voice-pip")
			.absolute()
			.top(theme.title_bar_height + theme.top_bar_height + theme.space_8)
			.right(theme.space_16)
			.w(width)
			.h(width * PIP_ASPECT)
			.overflow_hidden()
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.cursor_pointer()
			.occlude()
			.on_click(cx.listener(move |this, _, _, cx| {
				this.data
					.read(cx)
					.send(Command::SelectChannel { guild_id, channel_id });
			}))
			.when_some(image, |pip, image| {
				pip.child(img(image).size_full().object_fit(ObjectFit::Contain))
			})
			.child(
				div()
					.absolute()
					.top(theme.space_4)
					.right(theme.space_4)
					.flex()
					.gap(theme.space_4)
					.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
					.child(
						IconButton::new("voice-pip-popout", IconKind::AppWindow)
							.filled_hover()
							.color(theme.interactive_active)
							.tooltip(cx.t("voice-popout"))
							.on_click(cx.listener(move |this, _, _, cx| popout::open(&this.data, owner, cx))),
					)
					.child(
						IconButton::new("voice-pip-close", IconKind::Close)
							.filled_hover()
							.color(theme.interactive_active)
							.tooltip(cx.t("voice-stop-watching"))
							.on_click(cx.listener(move |this, _, _, cx| {
								this.data.read(cx).send(Command::StopWatchingStream {
									stream_key: stream_key.clone(),
								});
							})),
					),
			)
			.into_any_element()
	}
}
