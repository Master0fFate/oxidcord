use std::rc::Rc;

use gpui::{
	AnyElement, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render, SharedString,
	Styled as _, Subscription, Window, div,
};
use gpui_component::switch::Switch;
use protocol::{MediaRedaction, MediaScope, PrivacyChange, Snowflake};
use session::Command;

use super::widgets::{heading, row};
use crate::components::{PickHandler, select};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::theme::Theme;

const REDACTIONS: [(MediaRedaction, &str); 3] = [
	(MediaRedaction::Show, "privacy-media-show"),
	(MediaRedaction::Blur, "privacy-media-blur"),
	(MediaRedaction::Block, "privacy-media-block"),
];
const MEDIA_SCOPES: [(MediaScope, &str); 3] = [
	(MediaScope::Guild, "privacy-media-guilds"),
	(MediaScope::FriendDm, "privacy-media-friends"),
	(MediaScope::NonFriendDm, "privacy-media-others"),
];
const VISIBILITIES: [(u32, &str); 3] = [
	(1, "privacy-visibility-friends"),
	(2, "privacy-visibility-small"),
	(3, "privacy-visibility-all"),
];
const DEFAULT_VISIBILITY: u32 = 3;

pub struct PrivacyPage {
	data: Entity<Data>,
	guilds: Vec<(Snowflake, SharedString)>,
	_subscription: Subscription,
}

fn guild_list(data: &Entity<Data>, cx: &Context<PrivacyPage>) -> Vec<(Snowflake, SharedString)> {
	data.read(cx)
		.state()
		.guild_summaries()
		.into_iter()
		.map(|summary| (summary.id, summary.name.into()))
		.collect()
}

impl PrivacyPage {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |this, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.user_settings || changes.guild_list)
			{
				this.guilds = guild_list(&this.data, cx);
				cx.notify();
			}
		});
		Self {
			guilds: guild_list(&data, cx),
			data,
			_subscription: subscription,
		}
	}

	fn change(&self, change: PrivacyChange, cx: &Context<Self>) {
		self.data.read(cx).send(Command::SetPrivacy(change));
	}

	fn guild_row(
		&self,
		index: usize,
		guild_id: Snowflake,
		name: &SharedString,
		open: (bool, bool),
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let dms = Switch::new(("privacy-guild-dms", index))
			.checked(open.0)
			.on_change(cx.listener(move |this, checked: &bool, _, cx| {
				this.change(
					PrivacyChange::GuildDms {
						guild_id,
						restricted: !*checked,
					},
					cx,
				);
			}));
		let requests = Switch::new(("privacy-guild-requests", index))
			.checked(open.1)
			.on_change(cx.listener(move |this, checked: &bool, _, cx| {
				this.change(
					PrivacyChange::GuildRequests {
						guild_id,
						restricted: !*checked,
					},
					cx,
				);
			}));
		row(
			name.clone(),
			None,
			div()
				.flex()
				.items_center()
				.gap(theme.space_16)
				.child(labeled(cx.t("privacy-dms-short"), dms, theme))
				.child(labeled(cx.t("privacy-requests-short"), requests, theme)),
			theme,
		)
	}

	fn media_row(
		&self,
		scope: MediaScope,
		label: &'static str,
		current: MediaRedaction,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let labels: Vec<SharedString> = REDACTIONS.iter().map(|(_, key)| cx.t(key)).collect();
		let selected = REDACTIONS
			.iter()
			.position(|(redaction, _)| *redaction == current)
			.unwrap_or_default();
		let data = self.data.clone();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			data.read(cx).send(Command::SetExplicitMedia {
				scope,
				redaction: REDACTIONS[index].0,
			});
		});
		row(
			cx.t(label),
			None,
			select(label, labels[selected].clone(), &labels, selected, pick),
			theme,
		)
	}

	fn visibility_row(&self, current: u32, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let labels: Vec<SharedString> = VISIBILITIES.iter().map(|(_, key)| cx.t(key)).collect();
		let current = if current == 0 { DEFAULT_VISIBILITY } else { current };
		let selected = VISIBILITIES
			.iter()
			.position(|(value, _)| *value == current)
			.unwrap_or_default();
		let data = self.data.clone();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			data.read(cx)
				.send(Command::SetPrivacy(PrivacyChange::ProfileVisibility(
					VISIBILITIES[index].0,
				)));
		});
		row(
			cx.t("privacy-visibility"),
			Some(cx.t("privacy-visibility-note")),
			select(
				"privacy-visibility",
				labels[selected].clone(),
				&labels,
				selected,
				pick,
			),
			theme,
		)
	}
}

fn labeled(label: SharedString, switch: Switch, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_8)
		.child(
			div()
				.text_size(theme.font_sm)
				.font_weight(FontWeight::MEDIUM)
				.text_color(theme.text_muted)
				.child(label),
		)
		.child(switch)
		.into_any_element()
}

impl Render for PrivacyPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (privacy, explicit) = {
			let data = self.data.read(cx);
			let state = data.state();
			(state.privacy().clone(), state.explicit_media())
		};
		let guild_rows: Vec<AnyElement> = self
			.guilds
			.iter()
			.enumerate()
			.map(|(index, (guild_id, name))| {
				let open = (
					!privacy.restricted_guilds.contains(guild_id),
					!privacy.request_restricted_guilds.contains(guild_id),
				);
				self.guild_row(index, *guild_id, name, open, &theme, cx)
			})
			.collect();
		let media_rows: Vec<AnyElement> = MEDIA_SCOPES
			.iter()
			.map(|(scope, label)| self.media_row(*scope, label, explicit.at(*scope), &theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.flex_col()
					.child(heading(cx.t("privacy-messages"), &theme))
					.child(row(
						cx.t("privacy-dms-default"),
						Some(cx.t("privacy-dms-default-note")),
						Switch::new("privacy-dms-default")
							.checked(!privacy.default_guilds_restricted)
							.on_change(cx.listener(|this, checked: &bool, _, cx| {
								this.change(
									PrivacyChange::DefaultGuildDms {
										restricted: !*checked,
									},
									cx,
								);
							})),
						&theme,
					))
					.child(row(
						cx.t("privacy-requests-default"),
						Some(cx.t("privacy-requests-default-note")),
						Switch::new("privacy-requests-default")
							.checked(!privacy.default_request_restricted)
							.on_change(cx.listener(|this, checked: &bool, _, cx| {
								this.change(
									PrivacyChange::DefaultRequests {
										restricted: !*checked,
									},
									cx,
								);
							})),
						&theme,
					)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.child(heading(cx.t("privacy-media"), &theme))
					.children(media_rows),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.child(heading(cx.t("privacy-profile"), &theme))
					.child(self.visibility_row(privacy.profile_visibility, &theme, cx)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.child(heading(cx.t("privacy-servers"), &theme))
					.children(guild_rows),
			)
	}
}
