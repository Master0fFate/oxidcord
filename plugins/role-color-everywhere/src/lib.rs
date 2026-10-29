use plugin_sdk::model::{Color as ApiColor, DisplayOverrideFull, MessageFull, MessageMeta, Presence};
use plugin_sdk::patch::{Patch, PatchExt};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Color, Event, Md, Plugin, PluginExt, User, data, export_plugin, storage};

const SETTING_KEYS: [&str; 4] = ["chatMentions", "names", "colorChatMessages", "messageSaturation"];
const DEFAULT_SATURATION: u32 = 30;
const MENTION_BACKGROUND_ALPHA: u32 = 0x33;
const OPAQUE: u32 = 0xff;

struct RoleColorEverywhere {
	chat_mentions: bool,
	names: bool,
	color_chat_messages: bool,
	message_saturation: u32,
}

impl Default for RoleColorEverywhere {
	fn default() -> Self {
		Self {
			chat_mentions: true,
			names: true,
			color_chat_messages: false,
			message_saturation: DEFAULT_SATURATION,
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	matches!(raw.trim(), "true" | "1" | "on")
}

fn parse_saturation(raw: &str) -> u32 {
	raw.trim()
		.parse::<f64>()
		.map_or(DEFAULT_SATURATION, |value| value.clamp(0., 100.) as u32)
}

fn rgba(color: u32, alpha: u32) -> u32 {
	color << 8 | alpha
}

impl RoleColorEverywhere {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			"chatMentions" => self.chat_mentions = parse_flag(value),
			"names" => self.names = parse_flag(value),
			"colorChatMessages" => self.color_chat_messages = parse_flag(value),
			"messageSaturation" => self.message_saturation = parse_saturation(value),
			_ => {}
		}
	}

	fn mention(&self, guild_id: u64, user_id: u64) -> Option<Md> {
		let member = data::member_info(guild_id, user_id).ok().flatten()?;
		let color = member.color.filter(|color| *color != 0)?;
		let name = member.nick.unwrap_or(member.user.display_name);
		Some(Md::Styled {
			children: vec![Md::text(format!("@{name}"))],
			color: Some(Color::Rgba(rgba(color, OPAQUE))),
			background: Some(Color::Rgba(rgba(color, MENTION_BACKGROUND_ALPHA))),
			mono: false,
		})
	}

	fn recolor(&self, guild_id: u64, nodes: Vec<Md>, changed: &mut bool) -> Vec<Md> {
		nodes
			.into_iter()
			.map(|node| match node {
				Md::UserMention(id) => match self.mention(guild_id, id) {
					Some(styled) => {
						*changed = true;
						styled
					}
					None => Md::UserMention(id),
				},
				Md::Bold(children) => Md::Bold(self.recolor(guild_id, children, changed)),
				Md::Italic(children) => Md::Italic(self.recolor(guild_id, children, changed)),
				Md::Underline(children) => Md::Underline(self.recolor(guild_id, children, changed)),
				Md::Strike(children) => Md::Strike(self.recolor(guild_id, children, changed)),
				Md::Spoiler(children) => Md::Spoiler(self.recolor(guild_id, children, changed)),
				Md::Quote(children) => Md::Quote(self.recolor(guild_id, children, changed)),
				Md::Subtext(children) => Md::Subtext(self.recolor(guild_id, children, changed)),
				Md::Heading { level, children } => Md::Heading {
					level,
					children: self.recolor(guild_id, children, changed),
				},
				Md::List {
					ordered,
					start,
					items,
				} => Md::List {
					ordered,
					start,
					items: items
						.into_iter()
						.map(|item| self.recolor(guild_id, item, changed))
						.collect(),
				},
				other => other,
			})
			.collect()
	}

	fn tint(&self, color: u32) -> Option<Patch> {
		if !self.color_chat_messages || self.message_saturation == 0 || color == 0 {
			return None;
		}
		let alpha = self.message_saturation * OPAQUE / 100;
		Some(Patch::keep().accent(Color::Rgba(rgba(color, alpha))))
	}
}

impl Plugin for RoleColorEverywhere {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"chatMentions":{"type":"boolean","title":"Chat mentions","description":"Show role colors in chat mentions","default":true},"names":{"type":"boolean","title":"Names","description":"Color names with the role color wherever the client shows a name","default":true},"colorChatMessages":{"type":"boolean","title":"Color chat messages","description":"Tint the stripe of a message with the role color of its author","default":false},"messageSaturation":{"type":"integer","title":"Message saturation","description":"Intensity of message coloring","minimum":0,"maximum":100,"default":30}}}"#
			.to_owned()
	}
}

impl PluginExt for RoleColorEverywhere {
	fn display_user_full(
		&mut self,
		guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		if !self.names {
			return None;
		}
		let color = data::member_info(guild_id?, user.id)
			.ok()
			.flatten()?
			.color
			.filter(|color| *color != 0)?;
		Some(DisplayOverrideFull {
			name: None,
			suffix: None,
			color: Some(ApiColor::Rgba(rgba(color, OPAQUE))),
			badges: Vec::new(),
			avatar: None,
			banner: None,
			decoration: None,
			pronouns: None,
		})
	}

	fn intercept_message_ext(&mut self, msg: &MessageFull, meta: &MessageMeta, content: Vec<Md>) -> PatchExt {
		let mut patch = meta
			.author_color
			.and_then(|color| self.tint(color))
			.unwrap_or_else(Patch::keep);
		if let Some(guild_id) = meta.guild_id
			&& self.chat_mentions
			&& !msg.mentions.is_empty()
		{
			let mut changed = false;
			let nodes = self.recolor(guild_id, content, &mut changed);
			if changed {
				patch = patch.content(nodes);
			}
		}
		PatchExt::keep().base(patch)
	}
}

export_plugin!(RoleColorEverywhere, ext);
