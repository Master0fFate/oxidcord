use std::rc::Rc;

use gpui::{
	App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
	Styled as _, Subscription, Window, div,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use protocol::{Channel, ChannelType, Snowflake, VideoQualityMode};
use session::{Command, VoiceChannelEdit};

use crate::components::{PickHandler, field, segmented, select};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::theme::{Theme, rem};

const MIN_BITRATE_KBPS: f32 = 8.;
const DEFAULT_BITRATE_KBPS: u32 = 64;
const MAX_VOICE_USER_LIMIT: f32 = 99.;
const MAX_STAGE_USER_LIMIT: f32 = 10_000.;
const STAGE_MAX_BITRATE_KBPS: f32 = 64.;
const VIP_MAX_BITRATE_KBPS: f32 = 384.;
const VIP_REGIONS: &str = "VIP_REGIONS";
const VALUE_WIDTH: f32 = 64.;
const QUALITIES: [(&str, VideoQualityMode); 2] = [
	("channel-quality-auto", VideoQualityMode::Auto),
	("channel-quality-full", VideoQualityMode::Full),
];

fn max_bitrate_kbps(kind: ChannelType, vip_regions: bool, premium_tier: u8) -> f32 {
	if kind == ChannelType::GuildStageVoice {
		return STAGE_MAX_BITRATE_KBPS;
	}
	if vip_regions {
		return VIP_MAX_BITRATE_KBPS;
	}
	match premium_tier {
		0 => 96.,
		1 => 128.,
		2 => 256.,
		_ => 384.,
	}
}

fn max_user_limit(kind: ChannelType) -> f32 {
	if kind == ChannelType::GuildStageVoice {
		MAX_STAGE_USER_LIMIT
	} else {
		MAX_VOICE_USER_LIMIT
	}
}

#[derive(Clone, PartialEq, Eq)]
struct Settings {
	bitrate_kbps: u32,
	user_limit: u32,
	region: Option<String>,
	quality: VideoQualityMode,
}

pub struct VoiceForm {
	data: Entity<Data>,
	bitrate: Entity<SliderState>,
	user_limit: Entity<SliderState>,
	region: Option<String>,
	quality: VideoQualityMode,
	original: Settings,
	regions_loaded: bool,
	_subscriptions: Vec<Subscription>,
}

impl VoiceForm {
	pub fn new(data: Entity<Data>, guild: Snowflake, channel: &Channel, cx: &mut Context<Self>) -> Self {
		let (premium_tier, vip_regions, regions_loaded) = {
			let data = data.read(cx);
			let state = data.state();
			let guild = state.guild(guild);
			(
				guild.map_or(0, |guild| guild.premium_tier),
				guild.is_some_and(|guild| guild.has_feature(VIP_REGIONS)),
				state.voice_regions().is_some(),
			)
		};
		if !regions_loaded {
			data.read(cx).send(Command::LoadVoiceRegions);
		}
		let max_bitrate = max_bitrate_kbps(channel.kind, vip_regions, premium_tier);
		let max_user_limit = max_user_limit(channel.kind);
		let stored_bitrate = channel
			.bitrate
			.and_then(|bps| u32::try_from(bps / 1000).ok())
			.filter(|kbps| *kbps > 0)
			.unwrap_or(DEFAULT_BITRATE_KBPS);
		let original = Settings {
			bitrate_kbps: stored_bitrate.clamp(MIN_BITRATE_KBPS as u32, max_bitrate as u32),
			user_limit: channel
				.user_limit
				.and_then(|limit| u32::try_from(limit).ok())
				.unwrap_or(0),
			region: channel.rtc_region.clone().filter(|region| !region.is_empty()),
			quality: channel.video_quality_mode.unwrap_or(VideoQualityMode::Auto),
		};
		let bitrate = cx.new(|_| {
			SliderState::new()
				.min(MIN_BITRATE_KBPS)
				.max(max_bitrate)
				.step(1.)
				.default_value(original.bitrate_kbps as f32)
		});
		let user_limit = cx.new(|_| {
			SliderState::new()
				.min(0.)
				.max(max_user_limit)
				.step(1.)
				.default_value((original.user_limit as f32).min(max_user_limit))
		});
		let subscriptions = vec![
			cx.subscribe(&bitrate, |_, _, _: &SliderEvent, cx| cx.notify()),
			cx.subscribe(&user_limit, |_, _, _: &SliderEvent, cx| cx.notify()),
			cx.subscribe(&data, |this, data, event: &DataEvent, cx| {
				if !this.regions_loaded
					&& matches!(event, DataEvent::Changed(_))
					&& data.read(cx).state().voice_regions().is_some()
				{
					this.regions_loaded = true;
					cx.notify();
				}
			}),
		];
		Self {
			data,
			bitrate,
			user_limit,
			region: original.region.clone(),
			quality: original.quality,
			original,
			regions_loaded,
			_subscriptions: subscriptions,
		}
	}

	fn current(&self, cx: &App) -> Settings {
		Settings {
			bitrate_kbps: self.bitrate.read(cx).value().end().round() as u32,
			user_limit: self.user_limit.read(cx).value().end().round() as u32,
			region: self.region.clone(),
			quality: self.quality,
		}
	}

	pub fn dirty(&self, cx: &App) -> bool {
		self.current(cx) != self.original
	}

	pub fn edit(&self, cx: &App) -> VoiceChannelEdit {
		let now = self.current(cx);
		let was = &self.original;
		VoiceChannelEdit {
			bitrate: (now.bitrate_kbps != was.bitrate_kbps).then(|| now.bitrate_kbps * 1000),
			user_limit: (now.user_limit != was.user_limit).then_some(now.user_limit),
			rtc_region: (now.region != was.region).then_some(now.region),
			video_quality_mode: (now.quality != was.quality).then_some(now.quality),
		}
	}

	fn region_options(&self, cx: &Context<Self>) -> (Vec<Option<String>>, Vec<SharedString>) {
		let mut ids = vec![None];
		let mut labels = vec![cx.t("channel-region-auto")];
		let data = self.data.read(cx);
		let state = data.state();
		for region in state.voice_regions().unwrap_or_default() {
			ids.push(Some(region.id.clone()));
			labels.push(region.name.clone().into());
		}
		if let Some(current) = self
			.region
			.as_ref()
			.filter(|id| !ids.contains(&Some((*id).clone())))
		{
			ids.push(Some(current.clone()));
			labels.push(current.clone().into());
		}
		(ids, labels)
	}

	fn slider_row(slider: &Entity<SliderState>, label: String, theme: &Theme) -> impl IntoElement {
		div()
			.flex()
			.items_center()
			.gap(theme.space_16)
			.child(div().flex_1().child(Slider::new(slider)))
			.child(
				div()
					.w(rem(VALUE_WIDTH / 16.))
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(label),
			)
	}
}

impl Render for VoiceForm {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let now = self.current(cx);
		let (ids, labels) = self.region_options(cx);
		let selected = ids.iter().position(|id| *id == self.region).unwrap_or(0);
		let weak = cx.weak_entity();
		let pick_region: PickHandler = {
			let weak = weak.clone();
			Rc::new(move |index, _, cx| {
				let region = ids.get(index).cloned().flatten();
				weak.update(cx, |this, cx| {
					this.region = region;
					cx.notify();
				})
				.ok();
			})
		};
		let quality_labels: Vec<SharedString> = QUALITIES.iter().map(|(key, _)| cx.t(key)).collect();
		let pick_quality: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.quality = QUALITIES[index].1;
				cx.notify();
			})
			.ok();
		});
		let limit_label = if now.user_limit == 0 {
			cx.t("channel-user-limit-none").to_string()
		} else {
			now.user_limit.to_string()
		};
		div()
			.flex()
			.flex_col()
			.gap(theme.space_20)
			.child(field(
				cx.t("channel-bitrate"),
				&theme,
				Self::slider_row(
					&self.bitrate,
					cx.t_args(
						"channel-bitrate-value",
						&[("kbps", ArgValue::from(now.bitrate_kbps as usize))],
					)
					.to_string(),
					&theme,
				),
			))
			.child(field(
				cx.t("channel-user-limit"),
				&theme,
				Self::slider_row(&self.user_limit, limit_label, &theme),
			))
			.child(field(
				cx.t("channel-region"),
				&theme,
				select(
					"channel-region",
					labels[selected].clone(),
					&labels,
					selected,
					pick_region,
				),
			))
			.child(field(
				cx.t("channel-video-quality"),
				&theme,
				segmented(
					"channel-video-quality",
					&quality_labels,
					QUALITIES
						.iter()
						.position(|(_, mode)| *mode == self.quality)
						.unwrap_or(0),
					&theme,
					pick_quality,
				),
			))
	}
}
