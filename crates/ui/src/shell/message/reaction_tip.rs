use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gpui::{
	AnyView, App, AppContext as _, Context, Entity, FontWeight, Global, IntoElement, ParentElement as _,
	Render, SharedString, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use protocol::Snowflake;
use session::Command;

use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::shell::community::data_handle;
use crate::theme::{Theme, rem};

const SHOWN: usize = 10;
const TIP_WIDTH: f32 = 220.;

#[derive(Default, Clone)]
pub struct ReactionRequests(pub Rc<RefCell<HashSet<(Snowflake, String)>>>);

impl Global for ReactionRequests {}

pub struct ReactionTip {
	data: Entity<Data>,
	guild: Option<Snowflake>,
	message: Snowflake,
	key: String,
	label: SharedString,
	count: i64,
	_subscription: Subscription,
}

pub fn build(
	channel: Snowflake,
	message: Snowflake,
	guild: Option<Snowflake>,
	key: String,
	label: SharedString,
	count: i64,
	cx: &mut App,
) -> AnyView {
	let data = data_handle(cx);
	let requested = cx
		.try_global::<ReactionRequests>()
		.is_some_and(|requests| requests.0.borrow_mut().insert((message, key.clone())));
	if requested {
		data.read(cx).send(Command::LoadReactionUsers {
			channel_id: channel,
			message_id: message,
			emoji: key.clone(),
		});
	}
	cx.new(|cx| {
		let subscription = cx.subscribe(&data, move |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.loaded.contains(message)
			{
				cx.notify();
			}
		});
		ReactionTip {
			data,
			guild,
			message,
			key,
			label,
			count,
			_subscription: subscription,
		}
	})
	.into()
}

impl Render for ReactionTip {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let names: Option<Vec<String>> = {
			let data = self.data.read(cx);
			let state = data.state();
			state.reaction_users(self.message, &self.key).map(|users| {
				users
					.iter()
					.take(SHOWN)
					.map(|user| state.display_name(self.guild, user.id))
					.collect()
			})
		};
		let remaining = names
			.as_ref()
			.map_or(0, |names| (self.count as usize).saturating_sub(names.len()));
		div()
			.flex()
			.flex_col()
			.gap(theme.space_4)
			.w(rem(TIP_WIDTH))
			.px(theme.space_12)
			.py(theme.space_8)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.text_size(theme.font_sm)
			.text_color(theme.text_normal)
			.child(
				div()
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(self.label.clone()),
			)
			.children(match names {
				Some(names) => names
					.into_iter()
					.map(|name| div().truncate().child(name).into_any_element())
					.collect::<Vec<_>>(),
				None => vec![
					div()
						.text_color(theme.text_muted)
						.child(cx.t("reaction-loading"))
						.into_any_element(),
				],
			})
			.when_some((remaining > 0).then_some(remaining), |card, remaining| {
				card.child(
					div()
						.text_color(theme.text_muted)
						.child(cx.t_args("reaction-more", &[("count", ArgValue::from(remaining))])),
				)
			})
	}
}
