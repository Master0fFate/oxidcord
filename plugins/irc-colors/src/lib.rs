use plugin_sdk::types::SettingChange;
use plugin_sdk::{DisplayOverride, Event, Plugin, PluginExt, User, data, export_plugin, storage};

const LIGHTNESS_KEY: &str = "lightness";
const ONLY_WITHOUT_COLOR_KEY: &str = "applyColorOnlyToUsersWithoutColor";
const ONLY_IN_DMS_KEY: &str = "applyColorOnlyInDms";
const DEFAULT_LIGHTNESS: f32 = 70.;

struct IrcColors {
	lightness: f32,
	only_without_color: bool,
	only_in_dms: bool,
}

impl Default for IrcColors {
	fn default() -> Self {
		Self {
			lightness: DEFAULT_LIGHTNESS,
			only_without_color: false,
			only_in_dms: false,
		}
	}
}

fn parse_lightness(raw: &str) -> f32 {
	raw.trim()
		.parse::<f32>()
		.map_or(DEFAULT_LIGHTNESS, |value| value.clamp(0., 100.))
}

fn parse_flag(raw: &str) -> bool {
	matches!(raw.trim(), "true" | "1" | "on")
}

fn mix(mut value: u64) -> u64 {
	value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
	value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
	value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
	value ^ (value >> 31)
}

fn hsl_hex(hue: f32, lightness: f32) -> String {
	let lightness = lightness / 100.;
	let chroma = 1. - (2. * lightness - 1.).abs();
	let sector = hue / 60.;
	let second = chroma * (1. - (sector % 2. - 1.).abs());
	let (red, green, blue) = match sector as u32 {
		0 => (chroma, second, 0.),
		1 => (second, chroma, 0.),
		2 => (0., chroma, second),
		3 => (0., second, chroma),
		4 => (second, 0., chroma),
		_ => (chroma, 0., second),
	};
	let offset = lightness - chroma / 2.;
	let byte = |channel: f32| ((channel + offset) * 255.).round() as u8;
	format!("#{:02x}{:02x}{:02x}", byte(red), byte(green), byte(blue))
}

fn has_role_color(guild_id: Option<u64>, user_id: u64) -> bool {
	guild_id
		.and_then(|guild_id| data::member_info(guild_id, user_id).ok().flatten())
		.and_then(|member| member.color)
		.is_some_and(|color| color != 0)
}

impl Plugin for IrcColors {
	fn init(&mut self) -> Result<(), String> {
		let read = |key: &str| {
			storage::get(&format!("settings.{key}"))
				.ok()
				.flatten()
				.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
		};
		if let Some(raw) = read(LIGHTNESS_KEY) {
			self.lightness = parse_lightness(&raw);
		}
		if let Some(raw) = read(ONLY_WITHOUT_COLOR_KEY) {
			self.only_without_color = parse_flag(&raw);
		}
		if let Some(raw) = read(ONLY_IN_DMS_KEY) {
			self.only_in_dms = parse_flag(&raw);
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		let Event::SettingChanged(SettingChange { key, value }) = event else {
			return;
		};
		match key.as_str() {
			LIGHTNESS_KEY => self.lightness = parse_lightness(&value),
			ONLY_WITHOUT_COLOR_KEY => self.only_without_color = parse_flag(&value),
			ONLY_IN_DMS_KEY => self.only_in_dms = parse_flag(&value),
			_ => {}
		}
	}

	fn display_user(&mut self, guild_id: Option<u64>, user: &User) -> Option<DisplayOverride> {
		if self.only_in_dms && guild_id.is_some() {
			return None;
		}
		if self.only_without_color && has_role_color(guild_id, user.id) {
			return None;
		}
		let hue = (mix(user.id) % 360) as f32;
		Some(DisplayOverride {
			name: None,
			color: Some(hsl_hex(hue, self.lightness)),
			badge: None,
		})
	}

	fn settings_schema(&self) -> String {
		r##"{"type":"object","properties":{"lightness":{"type":"number","title":"Lightness","description":"Lightness in percent. Change if the colors are too light or too dark","minimum":0,"maximum":100,"default":70},"applyColorOnlyToUsersWithoutColor":{"type":"boolean","title":"Only users without a role color","description":"Apply colors only to users who don't have a predefined color","default":false},"applyColorOnlyInDms":{"type":"boolean","title":"Only in direct messages","description":"Apply colors only in direct messages and not in servers","default":false}}}"##
			.to_owned()
	}
}

impl PluginExt for IrcColors {}

export_plugin!(IrcColors, ext);
