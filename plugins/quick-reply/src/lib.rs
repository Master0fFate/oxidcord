use plugin_sdk::model::{Keybind, Lifecycle, MessageFull, RelationshipKind};
use plugin_sdk::types::{MessageRef, SettingChange};
use plugin_sdk::{Event, Plugin, PluginExt, actions, client, data, export_plugin, host, log, storage};

const MENTION_KEY: &str = "shouldMention";
const IGNORE_BLOCKED_KEY: &str = "ignoreBlockedAndIgnored";
const SETTING_KEYS: [&str; 2] = [MENTION_KEY, IGNORE_BLOCKED_KEY];
const WINDOW: u32 = 100;

struct Binding {
	id: &'static str,
	chord: &'static str,
	editing: bool,
	previous: bool,
}

const BINDINGS: [Binding; 4] = [
	Binding {
		id: "reply-previous",
		chord: "ctrl+up",
		editing: false,
		previous: true,
	},
	Binding {
		id: "reply-next",
		chord: "ctrl+down",
		editing: false,
		previous: false,
	},
	Binding {
		id: "edit-previous",
		chord: "ctrl+shift+up",
		editing: true,
		previous: true,
	},
	Binding {
		id: "edit-next",
		chord: "ctrl+shift+down",
		editing: true,
		previous: false,
	},
];

#[derive(Clone, Copy, PartialEq)]
enum Mention {
	Plugin,
	Enabled,
	Disabled,
}

impl Mention {
	fn parse(raw: &str) -> Self {
		match raw.trim() {
			"enabled" => Self::Enabled,
			"disabled" => Self::Disabled,
			_ => Self::Plugin,
		}
	}

	fn pings(self) -> bool {
		self != Self::Disabled
	}
}

struct QuickReply {
	mention: Mention,
	ignore_blocked: bool,
	replying: Option<u64>,
	editing: Option<u64>,
}

impl Default for QuickReply {
	fn default() -> Self {
		Self {
			mention: Mention::Plugin,
			ignore_blocked: true,
			replying: None,
			editing: None,
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

fn blocked_users() -> Vec<u64> {
	data::list_relationships()
		.unwrap_or_default()
		.into_iter()
		.filter(|relationship| relationship.kind == RelationshipKind::Blocked)
		.map(|relationship| relationship.user.id)
		.collect()
}

fn neighbour(candidates: &[&MessageFull], current: Option<u64>, previous: bool) -> Option<usize> {
	let position = current.and_then(|id| candidates.iter().position(|message| message.view.id == id));
	match position {
		Some(index) if previous => index.checked_sub(1),
		Some(index) => Some(index + 1).filter(|&next| next < candidates.len()),
		None => candidates.len().checked_sub(1),
	}
}

impl QuickReply {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			MENTION_KEY => self.mention = Mention::parse(value),
			IGNORE_BLOCKED_KEY => self.ignore_blocked = parse_flag(value),
			_ => {}
		}
	}

	fn step(&mut self, editing: bool, previous: bool) {
		let Ok(Some((_, channel_id))) = host::current_channel() else {
			return;
		};
		let Ok(messages) = client::channel_messages(channel_id, WINDOW) else {
			return;
		};
		let blocked = if self.ignore_blocked {
			blocked_users()
		} else {
			Vec::new()
		};
		let candidates: Vec<&MessageFull> = messages
			.iter()
			.filter(|message| {
				!matches!(message.lifecycle, Lifecycle::Deleted(_))
					&& (!editing || message.mine)
					&& !blocked.contains(&message.view.author.id)
			})
			.collect();
		let current = if editing {
			&mut self.editing
		} else {
			&mut self.replying
		};
		let next = neighbour(&candidates, *current, previous);
		*current = next.map(|index| candidates[index].view.id);
		let Some(message) = next.map(|index| candidates[index]) else {
			return;
		};
		let target = MessageRef {
			id: message.view.id,
			channel_id,
			guild_id: message.view.guild_id,
		};
		let outcome = if editing {
			actions::start_edit(target)
		} else {
			actions::start_reply(target, self.mention.pings())
		};
		if let Err(error) = outcome {
			log::debug!("quick reply failed: {error:?}");
		}
	}
}

impl Plugin for QuickReply {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		for binding in &BINDINGS {
			let bind = Keybind {
				id: binding.id.to_owned(),
				chord: binding.chord.to_owned(),
			};
			if let Err(error) = actions::register_keybind(&bind) {
				log::warn!("could not bind {}: {error:?}", binding.chord);
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::SettingChanged(SettingChange { key, value }) => self.set(&key, &value),
			Event::ChannelSwitched(_) => {
				self.replying = None;
				self.editing = None;
			}
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"shouldMention":{"type":"string","title":"Ping reply by default","description":"plugin follows the No Reply Mention plugin when it is enabled, enabled always pings, disabled never pings","enum":["plugin","enabled","disabled"],"default":"plugin"},"ignoreBlockedAndIgnored":{"type":"boolean","title":"Ignore blocked users","description":"Skip messages of blocked users when navigating","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for QuickReply {
	fn on_keybind(&mut self, id: &str) {
		if let Some(binding) = BINDINGS.iter().find(|binding| binding.id == id) {
			self.step(binding.editing, binding.previous);
		}
	}
}

export_plugin!(QuickReply, ext);
