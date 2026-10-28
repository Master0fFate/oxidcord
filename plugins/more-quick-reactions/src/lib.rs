use std::collections::HashMap;

use plugin_sdk::model::{EmojiRef, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, MessageRef, Plugin, PluginExt, Reply, actions, export_plugin, host, log, mount, storage,
};

const COUNT_KEY: &str = "reactionCount";
const COUNT_STORAGE_KEY: &str = "settings.reactionCount";
const USAGE_KEY: &str = "usage";
const MOUNT_KEY: &str = "reactions";
const BUTTON_PREFIX: &str = "react:";
const DEFAULT_COUNT: usize = 4;
const MAX_COUNT: usize = 12;
const MAX_TRACKED: usize = 40;
const DEFAULTS: [&str; MAX_COUNT] = [
	"👍", "❤️", "😂", "😮", "😢", "🎉", "🔥", "👀", "💯", "🙏", "✅", "😍",
];

#[derive(Default)]
struct MoreQuickReactions {
	count: usize,
	usage: HashMap<String, u32>,
	slots: Vec<String>,
	own_id: Option<u64>,
}

fn clamp_count(raw: &str) -> usize {
	raw.trim()
		.parse::<f64>()
		.map_or(DEFAULT_COUNT, |count| count.clamp(0., MAX_COUNT as f64) as usize)
}

fn emoji_ref(key: &str) -> EmojiRef {
	match key.rsplit_once(':') {
		Some((name, id)) if id.parse::<u64>().is_ok() => EmojiRef {
			id: id.parse().ok(),
			name: name.to_owned(),
			animated: false,
		},
		_ => EmojiRef {
			id: None,
			name: key.to_owned(),
			animated: false,
		},
	}
}

fn label(key: &str) -> String {
	match key.rsplit_once(':') {
		Some((name, id)) if id.parse::<u64>().is_ok() => format!(":{name}:"),
		_ => key.to_owned(),
	}
}

impl MoreQuickReactions {
	fn load(&mut self) {
		self.count = storage::get(COUNT_STORAGE_KEY)
			.ok()
			.flatten()
			.map_or(DEFAULT_COUNT, |bytes| {
				clamp_count(&String::from_utf8_lossy(&bytes))
			});
		if let Ok(Some(saved)) = storage::get(USAGE_KEY) {
			self.usage = String::from_utf8_lossy(&saved)
				.lines()
				.filter_map(|line| {
					let (count, key) = line.split_once('\t')?;
					Some((key.to_owned(), count.parse().ok()?))
				})
				.collect();
		}
	}

	fn save_usage(&mut self) {
		if self.usage.len() > MAX_TRACKED {
			let mut ranked: Vec<_> = self.usage.drain().collect();
			ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
			ranked.truncate(MAX_TRACKED);
			self.usage = ranked.into_iter().collect();
		}
		let lines: Vec<String> = self
			.usage
			.iter()
			.map(|(key, count)| format!("{count}\t{key}"))
			.collect();
		if let Err(error) = storage::set(USAGE_KEY, lines.join("\n").as_bytes()) {
			log::warn!("could not save the reaction counts: {error:?}");
		}
	}

	fn choose_slots(&mut self) {
		let mut ranked: Vec<(&String, &u32)> = self.usage.iter().collect();
		ranked.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
		let mut slots: Vec<String> = Vec::with_capacity(self.count);
		for key in ranked.into_iter().map(|(key, _)| key.as_str()).chain(DEFAULTS) {
			if slots.len() == self.count {
				break;
			}
			if !slots.iter().any(|slot| slot == key) {
				slots.push(key.to_owned());
			}
		}
		self.slots = slots;
	}

	fn tree(&self) -> Vec<Node> {
		if self.slots.is_empty() {
			return Vec::new();
		}
		let buttons = self
			.slots
			.iter()
			.enumerate()
			.map(|(index, key)| view::button(format!("{BUTTON_PREFIX}{index}"), label(key)));
		vec![view::row().children(buttons)]
	}

	fn refresh(&mut self) {
		self.choose_slots();
		if let Err(error) = mount::update(mount::MESSAGE_ACTION, MOUNT_KEY, self.tree()) {
			log::warn!("could not update the quick reactions: {error:?}");
		}
	}

	fn count_own_reaction(&mut self, emoji: &str) {
		*self.usage.entry(emoji.to_owned()).or_insert(0) += 1;
		self.save_usage();
		self.refresh();
	}

	fn is_me(&mut self, user_id: u64) -> bool {
		if self.own_id.is_none() {
			self.own_id = host::current_user().ok().map(|me| me.id);
		}
		self.own_id == Some(user_id)
	}
}

impl Plugin for MoreQuickReactions {
	fn init(&mut self) -> Result<(), String> {
		self.load();
		self.choose_slots();
		let options = mount::Options::new("Quick reactions");
		if let Err(error) = mount::mount(mount::MESSAGE_ACTION, MOUNT_KEY, self.tree(), options) {
			log::warn!("could not add the quick reactions: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::SettingChanged(SettingChange { key, value }) if key == COUNT_KEY => {
				self.count = clamp_count(&value);
				self.refresh();
			}
			Event::ReactionAdded(reaction) if self.is_me(reaction.user_id) => {
				self.count_own_reaction(&reaction.emoji);
			}
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"reactionCount":{"type":"integer","title":"Reaction count","description":"Number of quick reactions on the message hover bar","minimum":0,"maximum":12,"default":4}}}"#
			.to_owned()
	}
}

impl PluginExt for MoreQuickReactions {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click {
			return Reply::none();
		}
		let slot = event
			.node
			.strip_prefix(BUTTON_PREFIX)
			.and_then(|index| index.parse::<usize>().ok())
			.and_then(|index| self.slots.get(index));
		if let (Some(key), Some(id), Some(channel_id)) =
			(slot, event.context.message_id, event.context.channel_id)
		{
			let message = MessageRef {
				id,
				channel_id,
				guild_id: event.context.guild_id,
			};
			if let Err(error) = actions::react(message, &emoji_ref(key), false) {
				log::warn!("could not react: {error:?}");
			}
		}
		Reply::none()
	}
}

export_plugin!(MoreQuickReactions, ext);
