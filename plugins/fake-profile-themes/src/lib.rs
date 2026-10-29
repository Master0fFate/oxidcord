use plugin_sdk::model::{
	ButtonVariant, Length, Radius, Space, TextSize, UiContext, UserProfile, ViewEvent, ViewEventKind,
};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Color, Event, HostError, Plugin, PluginExt, Reply, data, export_plugin, log, mount, storage,
};

const NITRO_FIRST_KEY: &str = "nitroFirst";
const LOAD_BUTTON: &str = "load";
const TITLE: &str = "Profile colors";
const SWATCH_PX: u32 = 20;
const TAG_FIRST: u32 = 0xE0000;
const TAG_LAST: u32 = 0xE007F;
const MAX_HEX_DIGITS: usize = 6;

struct FakeProfileThemes {
	nitro_first: bool,
}

impl Default for FakeProfileThemes {
	fn default() -> Self {
		Self { nitro_first: true }
	}
}

fn take_hex(text: &str) -> Option<(u32, &str)> {
	let length = text
		.bytes()
		.take(MAX_HEX_DIGITS)
		.take_while(u8::is_ascii_hexdigit)
		.count();
	let (digits, rest) = text.split_at(length);
	Some((u32::from_str_radix(digits, 16).ok()?, rest))
}

fn parse_pair(text: &str) -> Option<(u32, u32)> {
	let (primary, rest) = take_hex(text)?;
	let (accent, rest) = take_hex(rest.strip_prefix(",#")?)?;
	rest.starts_with(']').then_some((primary, accent))
}

fn decode(bio: &str) -> Option<(u32, u32)> {
	let hidden: String = bio
		.chars()
		.filter(|ch| (TAG_FIRST..=TAG_LAST).contains(&(*ch as u32)))
		.filter_map(|ch| char::from_u32(ch as u32 - TAG_FIRST))
		.collect();
	let mut rest = hidden.as_str();
	while let Some(start) = rest.find("[#") {
		rest = &rest[start + 2..];
		if let Some(pair) = parse_pair(rest) {
			return Some(pair);
		}
	}
	None
}

fn rgb_color(rgb: u32) -> Color {
	Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

fn ink_for(rgb: u32) -> Color {
	let (red, green, blue) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
	if (299 * red + 587 * green + 114 * blue) / 1000 > 150 {
		Color::rgb(0, 0, 0)
	} else {
		Color::rgb(255, 255, 255)
	}
}

fn swatch(rgb: u32, ink: &Color) -> Node {
	view::column()
		.width(Length::Px(SWATCH_PX))
		.height(Length::Px(SWATCH_PX))
		.radius(Radius::Full)
		.background(rgb_color(rgb))
		.border(ink.clone())
}

fn entry(rgb: u32, ink: &Color) -> Node {
	view::row()
		.gap(Space::Xs)
		.child(swatch(rgb, ink))
		.child(view::text(format!("#{rgb:06x}")).mono().color(ink.clone()))
}

fn colors_section((primary, accent): (u32, u32)) -> Node {
	let ink = ink_for(primary);
	view::column()
		.fill()
		.gap(Space::Sm)
		.padding(Space::Md)
		.radius(Radius::Md)
		.background(rgb_color(primary))
		.child(view::text(TITLE).bold().color(ink.clone()))
		.child(
			view::row()
				.gap(Space::Md)
				.child(entry(primary, &ink))
				.child(entry(accent, &ink)),
		)
}

fn load_prompt() -> Vec<Node> {
	vec![
		view::section(TITLE).child(
			view::button(LOAD_BUTTON, "Load")
				.icon("download")
				.variant(ButtonVariant::Ghost),
		),
	]
}

fn describe(error: &HostError) -> String {
	match error {
		HostError::RateLimited => "Too many requests, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "Discord took too long".to_owned(),
		HostError::Denied(_) | HostError::Unavailable => "Not available right now".to_owned(),
	}
}

impl FakeProfileThemes {
	fn colors_of(&self, profile: &UserProfile) -> Option<(u32, u32)> {
		if self.nitro_first && profile.accent_color.is_some() {
			return None;
		}
		decode(profile.bio.as_deref()?)
	}
}

impl Plugin for FakeProfileThemes {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{NITRO_FIRST_KEY}")) {
			self.nitro_first = !matches!(String::from_utf8_lossy(&saved).trim(), "false" | "0" | "off");
		}
		if let Err(error) = mount::provide(mount::USER_POPOUT_SECTION, mount::Options::new(TITLE)) {
			log::warn!("could not provide the popout section: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == NITRO_FIRST_KEY
		{
			self.nitro_first = !matches!(value.trim(), "false" | "0" | "off");
			if let Err(error) = mount::invalidate(mount::USER_POPOUT_SECTION) {
				log::debug!("could not refresh the popout section: {error:?}");
			}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"nitroFirst":{"type":"boolean","title":"Accent color first","description":"Skip the hidden colors when the profile already has an accent color","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for FakeProfileThemes {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		if point != mount::USER_POPOUT_SECTION {
			return None;
		}
		match data::user_profile(context.user_id?) {
			Ok(Some(profile)) => self
				.colors_of(&profile)
				.map(|colors| vec![colors_section(colors)]),
			Err(HostError::Unavailable) => Some(load_prompt()),
			_ => None,
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click || event.node != LOAD_BUTTON {
			return Reply::none();
		}
		let Some(user_id) = event.context.user_id else {
			return Reply::none();
		};
		match data::user_profile(user_id) {
			Ok(profile) => {
				let section = profile.and_then(|profile| self.colors_of(&profile)).map_or_else(
					|| view::section(TITLE).child(view::text("None").size(TextSize::Xs).muted()),
					colors_section,
				);
				Reply::none().replace([section])
			}
			Err(error) => Reply::none().toast(describe(&error)),
		}
	}
}

export_plugin!(FakeProfileThemes, ext);
