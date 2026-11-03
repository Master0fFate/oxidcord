use std::rc::Rc;

use gpui::{
	AnyElement, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render, SharedString,
	Styled as _, Subscription, Window, div,
};
use gpui_component::switch::Switch;
use protocol::{MediaCategory, MediaRedaction, MediaScope, PrivacyChange, Snowflake};
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
const MEDIA_CATEGORIES: [(MediaCategory, &str); 3] = [
	(MediaCategory::Explicit, "privacy-media-explicit"),
	(MediaCategory::Gore, "privacy-media-gore"),
	(MediaCategory::SelfHarm, "privacy-media-self-harm"),
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

struct GuildRow {
	id: Snowflake,
	name: SharedString,
	dms_open: bool,
	requests_open: bool,
}

pub struct PrivacyPage {
	data: Entity<Data>,
	guilds: Vec<GuildRow>,
	_subscription: Subscription,
}

fn guild_list(data: &Entity<Data>, cx: &Context<PrivacyPage>) -> Vec<GuildRow> {
	let data = data.read(cx);
	let state = data.state();
	let privacy = state.privacy();
	state
		.guild_summaries()
		.into_iter()
		.map(|summary| GuildRow {
			dms_open: !privacy.restricted_guilds.contains(&summary.id),
			requests_open: !privacy.request_restricted_guilds.contains(&summary.id),
			id: summary.id,
			name: summary.name.into(),
		})
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

	fn guild_row(&self, index: usize, guild: &GuildRow, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let guild_id = guild.id;
		let dms = Switch::new(("privacy-guild-dms", index))
			.checked(guild.dms_open)
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
			.checked(guild.requests_open)
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
			guild.name.clone(),
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
		category: MediaCategory,
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
			data.read(cx).send(Command::SetSensitiveMedia {
				category,
				scope,
				redaction: REDACTIONS[index].0,
			});
		});
		row(
			cx.t(label),
			None,
			select(
				format!("{label}-{category:?}"),
				labels[selected].clone(),
				&labels,
				selected,
				pick,
			),
			theme,
		)
	}

	fn media_section(
		&self,
		category: MediaCategory,
		title: &'static str,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let redactions = self.data.read(cx).state().media_redactions(category);
		let rows: Vec<AnyElement> = MEDIA_SCOPES
			.iter()
			.map(|(scope, label)| self.media_row(category, *scope, label, redactions.at(*scope), theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.child(heading(cx.t(title), theme))
			.children(rows)
			.into_any_element()
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
		let (dms_restricted, requests_restricted, visibility) = {
			let data = self.data.read(cx);
			let state = data.state();
			let privacy = state.privacy();
			(
				privacy.default_guilds_restricted,
				privacy.default_request_restricted,
				privacy.profile_visibility,
			)
		};
		let guild_rows: Vec<AnyElement> = self
			.guilds
			.iter()
			.enumerate()
			.map(|(index, guild)| self.guild_row(index, guild, &theme, cx))
			.collect();
		let media_sections: Vec<AnyElement> = MEDIA_CATEGORIES
			.iter()
			.map(|(category, title)| self.media_section(*category, title, &theme, cx))
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
							.checked(!dms_restricted)
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
							.checked(!requests_restricted)
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
			.children(media_sections)
			.child(
				div()
					.flex()
					.flex_col()
					.child(heading(cx.t("privacy-profile"), &theme))
					.child(self.visibility_row(visibility, &theme, cx)),
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
