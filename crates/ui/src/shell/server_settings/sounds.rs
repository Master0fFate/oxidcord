use gpui::{
	AnyElement, App, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render, Styled as _,
	Window, div, prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{Disableable as _, WindowExt as _};
use protocol::{SOUND_NAME_MAX, Snowflake, SoundboardSound};
use session::Command;

use super::{kit, sound_edit};
use crate::components::{IconButton, StatusEmoji};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::community::files::pick_sound;
use crate::theme::Theme;

pub struct SoundsPage {
	data: Entity<Data>,
	guild: Snowflake,
}

fn stem(name: &str) -> String {
	name.rsplit_once('.')
		.map_or(name, |(stem, _)| stem)
		.chars()
		.take(SOUND_NAME_MAX)
		.collect()
}

impl SoundsPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, cx: &App) -> Self {
		data.read(cx).send(Command::LoadGuildSounds { guild_id: guild });
		Self { data, guild }
	}

	fn sounds(&self, cx: &App) -> Vec<SoundboardSound> {
		self.data
			.read(cx)
			.state()
			.guild_sounds(self.guild)
			.map(<[SoundboardSound]>::to_vec)
			.unwrap_or_default()
	}

	fn confirm_delete(&mut self, id: Snowflake, name: String, window: &mut Window, cx: &mut Context<Self>) {
		let data = self.data.clone();
		let guild = self.guild;
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			alert
				.title(cx.t("sounds-delete-title"))
				.description(cx.t_args("sounds-delete-body", &[("name", ArgValue::from(name.clone()))]))
				.confirm()
				.ok_text(cx.t("sounds-delete"))
				.ok_variant(ButtonVariant::Danger)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::DeleteSound {
						guild_id: guild,
						sound_id: id,
					});
					true
				})
		});
	}

	fn card(&self, sound: &SoundboardSound, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = sound.sound_id;
		let name = sound.name.clone();
		let editing = sound.clone();
		let percent = (sound.volume * 100.).round() as u32;
		let changeable = self.data.read(cx).state().may_change_sound(self.guild, sound);
		div()
			.flex()
			.flex_col()
			.items_center()
			.gap(theme.space_8)
			.w(theme.manage_grid_cell + theme.space_32)
			.p(theme.space_12)
			.rounded(theme.radius_md)
			.bg(theme.background_secondary)
			.child(match StatusEmoji::from_sound(sound) {
				Some(emoji) => emoji.element(theme.portrait_lg, cx),
				None => Icon::new(IconKind::Soundboard)
					.size(theme.portrait_lg)
					.color(theme.interactive_muted)
					.into_any_element(),
			})
			.child(
				div()
					.w_full()
					.truncate()
					.text_center()
					.text_size(theme.font_md)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(sound.name.clone()),
			)
			.child(
				div()
					.text_size(theme.font_tiny)
					.text_color(theme.text_muted)
					.child(format!("{percent}%")),
			)
			.when(changeable, |card| {
				card.child(
					div()
						.flex()
						.items_center()
						.gap(theme.space_4)
						.child(
							IconButton::new(("sound-edit", id.get()), IconKind::Edit)
								.tooltip(cx.t("sounds-edit"))
								.on_click(cx.listener(move |this, _, window, cx| {
									sound_edit::open(
										window,
										cx,
										this.data.clone(),
										this.guild,
										editing.clone(),
									)
								})),
						)
						.child(
							IconButton::new(("sound-delete", id.get()), IconKind::Trash)
								.color(theme.danger)
								.tooltip(cx.t("sounds-delete"))
								.on_click(cx.listener(move |this, _, window, cx| {
									this.confirm_delete(id, name.clone(), window, cx)
								})),
						),
				)
			})
			.into_any_element()
	}
}

impl Render for SoundsPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let sounds = self.sounds(cx);
		let count = sounds.len();
		let limit = self.data.read(cx).state().expression_limits(self.guild).sounds;
		let cards: Vec<AnyElement> = sounds.iter().map(|sound| self.card(sound, &theme, cx)).collect();
		let empty = cards.is_empty();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-soundboard"), &theme))
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.justify_between()
					.pb(theme.space_16)
					.child(kit::count_caption(
						cx.t_args(
							"sounds-slots",
							&[("used", ArgValue::from(count)), ("total", ArgValue::from(limit))],
						),
						&theme,
					))
					.child(
						Button::new("sound-upload")
							.primary()
							.disabled(count >= limit)
							.icon(gpui_component::Icon::default().path(IconKind::Upload.path()))
							.label(cx.t("sounds-upload"))
							.on_click(cx.listener(|_, _, _, cx| {
								pick_sound(cx, |this: &mut Self, name, bytes, cx| {
									this.data.read(cx).send(Command::CreateSound {
										guild_id: this.guild,
										name: stem(&name),
										file: bytes,
									});
								})
							})),
					),
			)
			.child(
				div()
					.flex()
					.flex_wrap()
					.content_start()
					.gap(theme.space_12)
					.pb(theme.space_32)
					.children(cards)
					.when(empty, |grid| {
						grid.child(kit::empty(IconKind::Soundboard, cx.t("sounds-empty"), &theme))
					})
					.overflow_y_scrollbar()
					.id("sounds-grid")
					.flex_1()
					.min_h_0(),
			)
	}
}
