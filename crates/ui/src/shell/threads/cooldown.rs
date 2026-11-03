use std::time::Duration;

use gpui::{AnyElement, App, Context, Entity, IntoElement as _, ParentElement as _, Styled as _, Task, div};
use protocol::Snowflake;

use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::composer::countdown;
use crate::theme::{Theme, rem};

const TICK: Duration = Duration::from_secs(1);

pub fn remaining(data: &Entity<Data>, parent: Snowflake, cx: &App) -> Option<Duration> {
	data.read(cx).state().thread_cooldown_remaining(parent)
}

pub fn watch<V: 'static>(data: Entity<Data>, parent: Snowflake, cx: &mut Context<V>) -> Option<Task<()>> {
	remaining(&data, parent, cx)?;
	Some(cx.spawn(async move |this, cx| {
		loop {
			cx.background_executor().timer(TICK).await;
			let waiting = data.read_with(cx, |data, _| {
				data.state().thread_cooldown_remaining(parent).is_some()
			});
			if this.update(cx, |_, cx| cx.notify()).is_err() || !waiting {
				break;
			}
		}
	}))
}

pub fn line(remaining: Duration, theme: &Theme, cx: &App) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_4)
		.text_size(theme.font_sm)
		.text_color(theme.warning)
		.child(Icon::new(IconKind::Timer).size(rem(12.)))
		.child(cx.t_args(
			"composer-slowmode",
			&[("time", ArgValue::from(countdown(remaining)))],
		))
		.into_any_element()
}
