use std::collections::HashMap;

use plugin_sdk::model::MessageFull;
use plugin_sdk::patch::Patch;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Color, Event, Md, Plugin, PluginExt, data, export_plugin, storage};

const SHOW_AT_KEY: &str = "showAtSymbol";
const SHOW_AT_STORAGE_KEY: &str = "settings.showAtSymbol";
const AVATAR_SIZE: u32 = 16;
const CACHE_CAPACITY: usize = 512;

struct Profile {
	avatar_url: String,
	name: String,
}

struct MentionAvatars {
	show_at_symbol: bool,
	profiles: HashMap<u64, Profile>,
}

impl Default for MentionAvatars {
	fn default() -> Self {
		Self {
			show_at_symbol: true,
			profiles: HashMap::new(),
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

fn avatar_url(id: u64, hash: Option<&str>) -> String {
	match hash {
		Some(hash) => format!("https://cdn.discordapp.com/avatars/{id}/{hash}.webp?size=32"),
		None => format!("https://cdn.discordapp.com/embed/avatars/{}.png", (id >> 22) % 6),
	}
}

impl MentionAvatars {
	fn profile(&mut self, id: u64) -> Option<&Profile> {
		if !self.profiles.contains_key(&id) {
			let user = data::fetch_user(id).ok().flatten()?;
			if self.profiles.len() >= CACHE_CAPACITY {
				self.profiles.clear();
			}
			self.profiles.insert(
				id,
				Profile {
					avatar_url: avatar_url(id, user.avatar.as_deref()),
					name: user.display_name,
				},
			);
		}
		self.profiles.get(&id)
	}

	fn decorate(&mut self, nodes: Vec<Md>, changed: &mut bool) -> Vec<Md> {
		nodes
			.into_iter()
			.flat_map(|node| match node {
				Md::UserMention(id) => self.mention(id, changed),
				Md::Bold(children) => vec![Md::Bold(self.decorate(children, changed))],
				Md::Italic(children) => vec![Md::Italic(self.decorate(children, changed))],
				Md::Underline(children) => vec![Md::Underline(self.decorate(children, changed))],
				Md::Strike(children) => vec![Md::Strike(self.decorate(children, changed))],
				Md::Spoiler(children) => vec![Md::Spoiler(self.decorate(children, changed))],
				Md::Quote(children) => vec![Md::Quote(self.decorate(children, changed))],
				Md::Subtext(children) => vec![Md::Subtext(self.decorate(children, changed))],
				Md::Heading { level, children } => vec![Md::Heading {
					level,
					children: self.decorate(children, changed),
				}],
				Md::List {
					ordered,
					start,
					items,
				} => vec![Md::List {
					ordered,
					start,
					items: items
						.into_iter()
						.map(|item| self.decorate(item, changed))
						.collect(),
				}],
				other => vec![other],
			})
			.collect()
	}

	fn mention(&mut self, id: u64, changed: &mut bool) -> Vec<Md> {
		let show_at_symbol = self.show_at_symbol;
		let Some(profile) = self.profile(id) else {
			return vec![Md::UserMention(id)];
		};
		*changed = true;
		let avatar = Md::InlineImage {
			url: profile.avatar_url.clone(),
			alt: String::new(),
			width: AVATAR_SIZE,
			height: AVATAR_SIZE,
		};
		if show_at_symbol {
			return vec![avatar, Md::UserMention(id)];
		}
		vec![
			avatar,
			Md::Styled {
				children: vec![Md::text(profile.name.clone())],
				color: Some(Color::token("mention-text")),
				background: Some(Color::token("mention-background")),
				mono: false,
			},
		]
	}
}

impl Plugin for MentionAvatars {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(SHOW_AT_STORAGE_KEY) {
			self.show_at_symbol = parse_flag(&String::from_utf8_lossy(&saved));
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == SHOW_AT_KEY
		{
			self.show_at_symbol = parse_flag(&value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"showAtSymbol":{"type":"boolean","title":"Show @ symbol","description":"Whether the @ symbol is displayed on user mentions","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for MentionAvatars {
	fn intercept_message(&mut self, msg: &MessageFull, content: Vec<Md>) -> Patch {
		if msg.mentions.is_empty() {
			return Patch::keep();
		}
		let mut changed = false;
		let nodes = self.decorate(content, &mut changed);
		if changed {
			Patch::keep().content(nodes)
		} else {
			Patch::keep()
		}
	}
}

export_plugin!(MentionAvatars, ext);
