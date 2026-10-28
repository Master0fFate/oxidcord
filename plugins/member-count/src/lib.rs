use plugin_sdk::model::{Align, Space, TextSize, Tone, UiContext};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Event, Plugin, PluginExt, data, export_plugin, host, log, mount, storage, timer};

const MEMBER_LIMIT: u32 = 1000;
const REFRESH_MS: u32 = 30_000;
const REFRESH_TAG: &str = "refresh";
const POINTS: [&str; 2] = [mount::MEMBER_LIST_HEADER, mount::GUILD_TOOLTIP];
const SETTING_KEYS: [&str; 3] = ["toolTip", "memberList", "voiceActivity"];

struct Config {
	tooltip: bool,
	member_list: bool,
	voice_activity: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			tooltip: true,
			member_list: true,
			voice_activity: true,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let on = !matches!(value.trim(), "false" | "0" | "off");
		match key {
			"toolTip" => self.tooltip = on,
			"memberList" => self.member_list = on,
			"voiceActivity" => self.voice_activity = on,
			_ => {}
		}
	}
}

#[derive(Default)]
struct MemberCount {
	config: Config,
}

fn grouped(count: usize) -> String {
	let digits = count.to_string();
	let mut out = String::with_capacity(digits.len() + digits.len() / 3);
	for (index, digit) in digits.chars().enumerate() {
		if index > 0 && (digits.len() - index).is_multiple_of(3) {
			out.push(',');
		}
		out.push(digit);
	}
	out
}

fn online_count(guild_id: u64) -> Option<String> {
	let members = host::list_members(guild_id, MEMBER_LIMIT).ok()?;
	if members.is_empty() {
		return None;
	}
	let online = members
		.iter()
		.filter(|member| !matches!(member.status.as_str(), "" | "offline" | "invisible"))
		.count();
	let capped = members.len() >= MEMBER_LIMIT as usize;
	Some(format!("{}{}", grouped(online), if capped { "+" } else { "" }))
}

fn voice_count(guild_id: u64) -> Option<usize> {
	data::voice_detail()
		.ok()
		.flatten()
		.filter(|call| call.base.guild_id == Some(guild_id))
		.map(|call| call.base.participants.len())
}

fn stat(icon: &str, tone: Tone, value: String, hint: &str) -> Node {
	view::row()
		.gap(Space::Xs)
		.align(Align::Center)
		.tooltip(hint)
		.child(view::icon(icon).tone(tone))
		.child(view::text(value).size(TextSize::Xs).bold())
}

fn counts(guild_id: u64, with_voice: bool) -> Option<Vec<Node>> {
	let total = data::guild_detail(guild_id).ok().flatten()?.member_count?;
	let online = online_count(guild_id).unwrap_or_else(|| "?".to_owned());
	let mut row = view::row()
		.gap(Space::Md)
		.align(Align::Center)
		.child(stat("circle", Tone::Success, online, "Online in the member list"))
		.child(stat(
			"users",
			Tone::Muted,
			grouped(total as usize),
			"Members in the server",
		));
	if with_voice && let Some(voice) = voice_count(guild_id) {
		row = row.child(stat(
			"volume-2",
			Tone::Muted,
			grouped(voice),
			"In your voice call",
		));
	}
	Some(vec![row])
}

impl MemberCount {
	fn refresh(&self) {
		for point in POINTS {
			if let Err(error) = mount::invalidate(point) {
				log::debug!("could not refresh {point}: {error:?}");
			}
		}
	}
}

impl Plugin for MemberCount {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		for point in POINTS {
			if let Err(error) = mount::provide(point, mount::Options::new("Member count")) {
				log::warn!("could not provide {point}: {error:?}");
			}
		}
		if let Err(error) = timer::every(REFRESH_MS, REFRESH_TAG) {
			log::warn!("could not start the refresh timer: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::SettingChanged(SettingChange { key, value }) => {
				self.config.set(&key, &value);
				self.refresh();
			}
			Event::VoiceChanged(_) if self.config.voice_activity => {
				if let Err(error) = mount::invalidate(mount::MEMBER_LIST_HEADER) {
					log::debug!("could not refresh the member list header: {error:?}");
				}
			}
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"toolTip":{"type":"boolean","title":"Server tooltip","description":"Show member count on the server tooltip","default":true},"memberList":{"type":"boolean","title":"Member list","description":"Show member count in the member list","default":true},"voiceActivity":{"type":"boolean","title":"Voice activity","description":"Show voice activity with member count in the member list","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for MemberCount {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		let guild_id = context.guild_id?;
		match point {
			mount::MEMBER_LIST_HEADER if self.config.member_list => {
				counts(guild_id, self.config.voice_activity)
			}
			mount::GUILD_TOOLTIP if self.config.tooltip => counts(guild_id, false),
			_ => None,
		}
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == REFRESH_TAG {
			self.refresh();
		}
	}
}

export_plugin!(MemberCount, ext);
