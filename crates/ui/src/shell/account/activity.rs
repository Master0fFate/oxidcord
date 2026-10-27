use gpui::{
	AnyElement, App, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _, div,
	prelude::FluentBuilder as _,
};
use protocol::{Activity, ActivityKind, Snowflake};
use state::State;

use super::parts::section_label;
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::community::art::cover;
use crate::theme::{Theme, rem};

const ART_SIZE: f32 = 64.;
const BADGE_SIZE: f32 = 22.;
const ART_PIXELS: u32 = 160;

#[derive(Clone, PartialEq)]
pub struct ActivityView {
	kind: ActivityKind,
	name: SharedString,
	details: Option<SharedString>,
	state: Option<SharedString>,
	art: Option<String>,
	badge: Option<String>,
	start: Option<i64>,
	end: Option<i64>,
	pub missing_icon: Option<Snowflake>,
}

impl ActivityView {
	pub fn is_timed(&self) -> bool {
		self.start.is_some()
	}
}

fn shown(kind: ActivityKind) -> bool {
	matches!(
		kind,
		ActivityKind::Playing
			| ActivityKind::Streaming
			| ActivityKind::Listening
			| ActivityKind::Watching
			| ActivityKind::Competing
	)
}

fn text(value: &Option<String>) -> Option<SharedString> {
	value
		.as_deref()
		.filter(|value| !value.is_empty())
		.map(|value| SharedString::from(value.to_owned()))
}

fn view(activity: &Activity, state: &State) -> ActivityView {
	let application = activity.application_id;
	let asset = |pick: fn(&protocol::ActivityAssets) -> &Option<String>| {
		activity
			.assets
			.as_ref()
			.and_then(|assets| pick(assets).as_deref())
			.and_then(|asset| {
				media::cdn::activity_asset_url(application.map(Snowflake::get), asset, ART_PIXELS)
			})
	};
	let large = asset(|assets| &assets.large_image);
	let icon = application.and_then(|id| state.application_icon(id).map(|icon| (id, icon)));
	let art = large.clone().or_else(|| {
		let (id, hash) = icon?;
		Some(media::cdn::application_icon_url(id.get(), hash?, ART_PIXELS))
	});
	let missing_icon = application.filter(|_| large.is_none() && icon.is_none());
	let timestamps = activity.timestamps.as_ref();
	ActivityView {
		kind: activity.kind,
		name: activity.name.clone().into(),
		details: text(&activity.details),
		state: text(&activity.state),
		art,
		badge: asset(|assets| &assets.small_image),
		start: timestamps.and_then(|timestamps| timestamps.start),
		end: timestamps.and_then(|timestamps| timestamps.end),
		missing_icon,
	}
}

pub fn activities(state: &State, user: Snowflake) -> Vec<ActivityView> {
	state
		.presence(user)
		.map(|presence| {
			presence
				.activities
				.iter()
				.filter(|activity| shown(activity.kind))
				.map(|activity| view(activity, state))
				.collect()
		})
		.unwrap_or_default()
}

fn heading(activity: &ActivityView, cx: &App) -> SharedString {
	let name = [("name", ArgValue::from(activity.name.to_string()))];
	match activity.kind {
		ActivityKind::Streaming => cx.t("activity-streaming"),
		ActivityKind::Listening => cx.t_args("activity-listening", &name),
		ActivityKind::Watching => cx.t_args("activity-watching", &name),
		ActivityKind::Competing => cx.t_args("activity-competing", &name),
		_ => cx.t("activity-playing"),
	}
}

fn clock(activity: &ActivityView, now: i64, theme: &Theme) -> Option<AnyElement> {
	let start = activity.start?;
	let elapsed = (now - start) / 1000;
	let label = match activity.end {
		Some(end) if end > start => format!(
			"{} / {}",
			format::elapsed_clock(elapsed.min((end - start) / 1000)),
			format::elapsed_clock((end - start) / 1000)
		),
		_ => format::elapsed_clock(elapsed),
	};
	let icon = if activity.kind == ActivityKind::Listening {
		IconKind::Headphones
	} else {
		IconKind::Gamepad
	};
	Some(
		div()
			.flex()
			.items_center()
			.gap(theme.space_4)
			.text_size(theme.font_sm)
			.font_family(theme.code_font.clone())
			.text_color(theme.status_online)
			.child(Icon::new(icon).size(theme.glyph_sm).color(theme.status_online))
			.child(label)
			.into_any_element(),
	)
}

fn art(activity: &ActivityView, theme: &Theme, cx: &App) -> AnyElement {
	let side = rem(ART_SIZE);
	let main = match &activity.art {
		Some(url) => div()
			.size_full()
			.rounded(theme.radius_md)
			.overflow_hidden()
			.child(cover(url, side, side, gpui::ObjectFit::Cover, cx))
			.into_any_element(),
		None => div()
			.flex()
			.items_center()
			.justify_center()
			.size_full()
			.rounded(theme.radius_md)
			.bg(theme.background_tertiary)
			.child(
				Icon::new(IconKind::Gamepad)
					.size(rem(32.))
					.color(theme.interactive_normal),
			)
			.into_any_element(),
	};
	let badge = activity.badge.as_ref().map(|url| {
		div()
			.absolute()
			.right(rem(-4.))
			.bottom(rem(-4.))
			.size(rem(BADGE_SIZE))
			.rounded_full()
			.overflow_hidden()
			.border_2()
			.border_color(theme.background_floating)
			.child(cover(
				url,
				rem(BADGE_SIZE),
				rem(BADGE_SIZE),
				gpui::ObjectFit::Cover,
				cx,
			))
	});
	div()
		.relative()
		.flex_none()
		.size(side)
		.child(main)
		.children(badge)
		.into_any_element()
}

pub fn activity_card(activity: &ActivityView, now: i64, theme: &Theme, cx: &App) -> AnyElement {
	let line = |value: &Option<SharedString>| {
		value.clone().map(|value| {
			div()
				.truncate()
				.text_size(theme.font_sm)
				.text_color(theme.text_normal)
				.child(value)
		})
	};
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.p(theme.space_12)
		.rounded(theme.radius_md)
		.bg(theme.background_secondary)
		.child(section_label(heading(activity, cx), theme))
		.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_12)
				.child(art(activity, theme, cx))
				.child(
					div()
						.flex()
						.flex_col()
						.flex_1()
						.min_w_0()
						.gap(rem(2.))
						.child(
							div()
								.truncate()
								.text_size(theme.font_md)
								.font_weight(FontWeight::BOLD)
								.text_color(theme.header_primary)
								.child(activity.name.clone()),
						)
						.children(line(&activity.details))
						.children(line(&activity.state))
						.when_some(clock(activity, now, theme), |column, clock| column.child(clock)),
				),
		)
		.into_any_element()
}
