use std::rc::Rc;

use gpui::{
	AnyElement, App, ClickEvent, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
	SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
	relative,
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
const STACK_OFFSET: f32 = 6.;
const STACK_INSET: f32 = 10.;
const STACK_FADE: f32 = 0.3;
const MAX_STACKED: usize = 3;
const PROGRESS_HEIGHT: f32 = 4.;

pub type OpenLink = Rc<dyn Fn(&str, &mut Window, &mut App)>;

#[derive(Clone, PartialEq)]
pub struct ActivityView {
	kind: ActivityKind,
	name: SharedString,
	details: Option<SharedString>,
	details_url: Option<String>,
	state: Option<SharedString>,
	state_url: Option<String>,
	art: Option<String>,
	art_text: Option<SharedString>,
	art_url: Option<String>,
	badge: Option<String>,
	badge_url: Option<String>,
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

fn link(value: &Option<String>) -> Option<String> {
	value
		.as_deref()
		.filter(|value| value.starts_with("https://") || value.starts_with("http://"))
		.map(str::to_owned)
}

fn view(activity: &Activity, state: &State) -> ActivityView {
	let application = activity.application_id;
	let assets = activity.assets.as_ref();
	let asset = |pick: fn(&protocol::ActivityAssets) -> &Option<String>| {
		assets
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
		details_url: link(&activity.details_url),
		state: text(&activity.state),
		state_url: link(&activity.state_url),
		art,
		art_text: assets.and_then(|assets| text(&assets.large_text)),
		art_url: assets.and_then(|assets| link(&assets.large_url)),
		badge: asset(|assets| &assets.small_image),
		badge_url: assets.and_then(|assets| link(&assets.small_url)),
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

struct Span {
	elapsed: i64,
	total: Option<i64>,
}

fn span(activity: &ActivityView, now: i64) -> Option<Span> {
	let start = activity.start?;
	let elapsed = ((now - start) / 1000).max(0);
	let total = activity
		.end
		.filter(|end| *end > start)
		.map(|end| (end - start) / 1000);
	Some(Span {
		elapsed: total.map_or(elapsed, |total| elapsed.min(total)),
		total,
	})
}

fn clock(activity: &ActivityView, now: i64, theme: &Theme) -> Option<AnyElement> {
	let span = span(activity, now)?;
	let label = match span.total {
		Some(total) => format!(
			"{} / {}",
			format::elapsed_clock(span.elapsed),
			format::elapsed_clock(total)
		),
		None => format::elapsed_clock(span.elapsed),
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

fn linked(
	id: (&'static str, usize),
	url: &Option<String>,
	open: &OpenLink,
	body: impl IntoElement,
) -> AnyElement {
	let Some(url) = url.clone() else {
		return body.into_any_element();
	};
	let open = open.clone();
	div()
		.id(id)
		.cursor_pointer()
		.hover(|style| style.opacity(0.8))
		.on_click(move |_, window, cx| {
			cx.stop_propagation();
			open(&url, window, cx);
		})
		.child(body)
		.into_any_element()
}

fn line(
	id: (&'static str, usize),
	value: &Option<SharedString>,
	url: &Option<String>,
	open: &OpenLink,
	theme: &Theme,
) -> Option<AnyElement> {
	let value = value.clone()?;
	let text = div()
		.truncate()
		.text_size(theme.font_sm)
		.text_color(theme.text_normal)
		.when(url.is_some(), |text| text.underline())
		.child(value);
	Some(linked(id, url, open, text))
}

fn art(activity: &ActivityView, index: usize, open: &OpenLink, theme: &Theme, cx: &App) -> AnyElement {
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
		let badge = div()
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
			));
		div().absolute().right(rem(-4.)).bottom(rem(-4.)).child(linked(
			("activity-badge", index),
			&activity.badge_url,
			open,
			badge,
		))
	});
	div()
		.relative()
		.flex_none()
		.size(side)
		.child(linked(("activity-art", index), &activity.art_url, open, main))
		.children(badge)
		.into_any_element()
}

fn progress(activity: &ActivityView, now: i64, theme: &Theme) -> Option<AnyElement> {
	let span = span(activity, now)?;
	let Some(total) = span.total else {
		return clock(activity, now, theme);
	};
	let fraction = if total > 0 {
		(span.elapsed as f32 / total as f32).clamp(0., 1.)
	} else {
		0.
	};
	let stamp = |seconds: i64| {
		div()
			.flex_none()
			.text_size(theme.font_xs)
			.font_family(theme.code_font.clone())
			.text_color(theme.text_normal)
			.child(format::elapsed_clock(seconds))
	};
	Some(
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.child(stamp(span.elapsed))
			.child(
				div()
					.flex_1()
					.h(rem(PROGRESS_HEIGHT))
					.rounded_full()
					.bg(theme.divider)
					.overflow_hidden()
					.child(
						div()
							.h_full()
							.w(relative(fraction))
							.rounded_full()
							.bg(theme.header_primary),
					),
			)
			.child(stamp(total))
			.into_any_element(),
	)
}

pub fn activity_card_full(
	activity: &ActivityView,
	index: usize,
	now: i64,
	open: &OpenLink,
	theme: &Theme,
	cx: &App,
) -> AnyElement {
	let (title, title_url) = match &activity.details {
		Some(details) => (details.clone(), &activity.details_url),
		None => (activity.name.clone(), &None),
	};
	let title = div()
		.truncate()
		.text_size(theme.font_lg)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_primary)
		.when(title_url.is_some(), |title| title.underline())
		.child(title);
	let album = activity.art_text.clone().filter(|_| activity.details.is_some());
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.p(theme.space_12)
		.rounded(theme.radius_md)
		.bg(theme.background_secondary)
		.child(
			div()
				.text_size(theme.font_sm)
				.font_weight(FontWeight::MEDIUM)
				.text_color(theme.header_secondary)
				.child(heading(activity, cx)),
		)
		.child(
			div()
				.flex()
				.items_start()
				.gap(theme.space_12)
				.child(art(activity, index, open, theme, cx))
				.child(
					div()
						.flex()
						.flex_col()
						.flex_1()
						.min_w_0()
						.gap(rem(2.))
						.child(linked(("activity-title", index), title_url, open, title))
						.children(line(
							("activity-state", index),
							&activity.state,
							&activity.state_url,
							open,
							theme,
						))
						.children(line(("activity-album", index), &album, &None, open, theme))
						.when_some(progress(activity, now, theme), |column, progress| {
							column.child(div().pt(theme.space_4).child(progress))
						}),
				),
		)
		.into_any_element()
}

pub fn activity_deck(
	activities: &[ActivityView],
	now: i64,
	open: &OpenLink,
	theme: &Theme,
	cx: &App,
	on_open: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
	let Some(front) = activities.first() else {
		return div().into_any_element();
	};
	let hidden = activities.len() - 1;
	let stacked = hidden.min(MAX_STACKED);
	let layers = (1..=stacked).rev().map(|depth| {
		let depth = depth as f32;
		div()
			.absolute()
			.top(rem(STACK_OFFSET * (stacked as f32 - depth)))
			.left(rem(STACK_INSET * depth))
			.right(rem(STACK_INSET * depth))
			.bottom_0()
			.rounded(theme.radius_md)
			.bg(theme.background_secondary)
			.opacity(1. - STACK_FADE * depth)
	});
	let count = (hidden > 0).then(|| {
		div()
			.absolute()
			.top(theme.space_8)
			.right(theme.space_12)
			.px(theme.space_8)
			.rounded_full()
			.bg(theme.background_tertiary)
			.text_size(theme.font_xs)
			.font_weight(FontWeight::SEMIBOLD)
			.text_color(theme.text_muted)
			.child(format!("+{hidden}"))
	});
	div()
		.id("activity-deck")
		.relative()
		.pt(rem(STACK_OFFSET * stacked as f32))
		.cursor_pointer()
		.hover(|style| style.opacity(0.92))
		.on_click(on_open)
		.children(layers)
		.child(
			div()
				.relative()
				.child(activity_card(front, 0, now, open, theme, cx))
				.children(count),
		)
		.into_any_element()
}

pub fn activity_card(
	activity: &ActivityView,
	index: usize,
	now: i64,
	open: &OpenLink,
	theme: &Theme,
	cx: &App,
) -> AnyElement {
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
				.child(art(activity, index, open, theme, cx))
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
						.children(line(
							("activity-details", index),
							&activity.details,
							&activity.details_url,
							open,
							theme,
						))
						.children(line(
							("activity-state", index),
							&activity.state,
							&activity.state_url,
							open,
							theme,
						))
						.when_some(clock(activity, now, theme), |column, clock| column.child(clock)),
				),
		)
		.into_any_element()
}
