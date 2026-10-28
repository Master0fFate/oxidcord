use plugin_sdk::types::ChannelSwitch;
use plugin_sdk::{Event, Plugin, export_plugin, host, log, now_ms, storage};

const LAST_KEY: &str = "last-channel";
const SETTLE_MS: i64 = 15_000;

type Place = (Option<u64>, u64);

#[derive(Default)]
struct KeepCurrentChannel {
	pending: Option<Place>,
	settle_until_ms: i64,
}

impl KeepCurrentChannel {
	fn restore(&self) {
		let Some((guild_id, channel_id)) = self.pending else {
			return;
		};
		if let Err(error) = host::open_channel(guild_id, channel_id) {
			log::warn!("could not reopen channel {channel_id}: {error:?}");
		}
	}

	fn remember(&mut self, switch: ChannelSwitch) {
		let place = (switch.guild_id, switch.channel_id);
		if let Some(pending) = self.pending {
			if pending == place || now_ms() >= self.settle_until_ms {
				self.pending = None;
			} else {
				return;
			}
		}
		if let Err(error) = storage::set(LAST_KEY, encode(place).as_bytes()) {
			log::warn!("could not save the current channel: {error:?}");
		}
	}
}

fn encode((guild_id, channel_id): Place) -> String {
	match guild_id {
		Some(guild_id) => format!("{guild_id}:{channel_id}"),
		None => format!(":{channel_id}"),
	}
}

fn decode(raw: &str) -> Option<Place> {
	let (guild_id, channel_id) = raw.split_once(':')?;
	let guild_id = if guild_id.is_empty() {
		None
	} else {
		Some(guild_id.parse().ok()?)
	};
	Some((guild_id, channel_id.parse().ok()?))
}

impl Plugin for KeepCurrentChannel {
	fn init(&mut self) -> Result<(), String> {
		self.pending = storage::get(LAST_KEY)
			.ok()
			.flatten()
			.and_then(|bytes| decode(&String::from_utf8_lossy(&bytes)));
		self.settle_until_ms = now_ms() + SETTLE_MS;
		self.restore();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::ChannelSwitched(switch) => self.remember(switch),
			Event::Connection(status) if status == "connected" => self.restore(),
			_ => {}
		}
	}
}

export_plugin!(KeepCurrentChannel);
