use gpui::{
	AnyElement, App, Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
	SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use protocol::{Snowflake, SoundboardSound};
use session::Command;
use state::State;

use crate::components::StatusEmoji;
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const PANEL_WIDTH: f32 = 30.;
const PANEL_MAX_HEIGHT: f32 = 24.;
const CELL_WIDTH: f32 = 9.;
const LOCKED_OPACITY: f32 = 0.5;

struct Entry {
	sound_id: Snowflake,
	name: SharedString,
	locked: bool,
	emoji: Option<StatusEmoji>,
}

struct Group {
	title: SharedString,
	entries: Vec<Entry>,
}

pub struct Board {
	groups: Vec<Group>,
}

fn group(title: &str, sounds: &[SoundboardSound], state: &State) -> Option<Group> {
	let entries: Vec<Entry> = sounds
		.iter()
		.filter(|sound| sound.usable())
		.map(|sound| Entry {
			sound_id: sound.sound_id,
			name: sound.name.clone().into(),
			locked: state.sound_block(sound).is_some(),
			emoji: StatusEmoji::from_sound(sound),
		})
		.collect();
	(!entries.is_empty()).then(|| Group {
		title: title.to_uppercase().into(),
		entries,
	})
}

impl Board {
	pub fn new(state: &State, guild: Snowflake, cx: &App) -> Self {
		let own = state
			.guild(guild)
			.and_then(|found| group(&found.name, state.guild_sounds(guild)?, state));
		let others = state
			.guilds()
			.filter(|other| other.id != guild && state.has_nitro())
			.filter_map(|other| group(&other.name, state.guild_sounds(other.id)?, state));
		let defaults = state
			.default_sounds()
			.and_then(|sounds| group(&cx.t("soundboard-default"), sounds, state));
		Self {
			groups: own.into_iter().chain(others).chain(defaults).collect(),
		}
	}
}

fn cell(entry: &Entry, channel: Snowflake, data: &Entity<Data>, theme: &Theme, cx: &App) -> AnyElement {
	let sound_id = entry.sound_id;
	let data = data.clone();
	div()
		.id(("voice-sound", sound_id.get()))
		.flex()
		.items_center()
		.gap(theme.space_8)
		.w(rem(CELL_WIDTH))
		.h(theme.control_height)
		.px(theme.space_8)
		.rounded(theme.radius_sm)
		.cursor_pointer()
		.hover(|style| style.bg(theme.modifier_hover))
		.when(entry.locked, |cell| cell.opacity(LOCKED_OPACITY))
		.on_click(move |_, _, cx| {
			data.read(cx).send(Command::SendSound {
				channel_id: channel,
				sound_id,
			})
		})
		.child(match &entry.emoji {
			Some(emoji) => emoji.element(theme.glyph_md, cx),
			None => Icon::new(IconKind::Soundboard)
				.size(theme.glyph_md)
				.color(theme.interactive_normal)
				.into_any_element(),
		})
		.child(
			div()
				.flex_1()
				.min_w_0()
				.truncate()
				.text_size(theme.font_sm)
				.text_color(theme.text_normal)
				.child(entry.name.clone()),
		)
		.into_any_element()
}

fn section(group: &Group, channel: Snowflake, data: &Entity<Data>, theme: &Theme, cx: &App) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.gap(theme.space_4)
		.child(
			div()
				.truncate()
				.text_size(theme.font_tiny)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_secondary)
				.child(group.title.clone()),
		)
		.child(
			div().flex().flex_wrap().children(
				group
					.entries
					.iter()
					.map(|entry| cell(entry, channel, data, theme, cx)),
			),
		)
		.into_any_element()
}

pub fn panel(board: &Board, data: &Entity<Data>, channel: Snowflake, theme: &Theme, cx: &App) -> AnyElement {
	let groups = &board.groups;
	div()
		.absolute()
		.left_0()
		.right_0()
		.bottom(theme.space_8)
		.flex()
		.justify_center()
		.child(
			div()
				.id("voice-soundboard")
				.occlude()
				.flex()
				.flex_col()
				.gap(theme.space_12)
				.w(rem(PANEL_WIDTH))
				.max_h(rem(PANEL_MAX_HEIGHT))
				.overflow_y_scroll()
				.p(theme.space_12)
				.rounded(theme.radius_md)
				.bg(theme.background_floating)
				.border_1()
				.border_color(theme.divider)
				.children(
					groups
						.iter()
						.map(|group| section(group, channel, data, theme, cx)),
				)
				.when(groups.is_empty(), |panel| {
					panel.child(
						div()
							.text_size(theme.font_md)
							.text_color(theme.text_muted)
							.child(cx.t("soundboard-empty")),
					)
				}),
		)
		.into_any_element()
}
