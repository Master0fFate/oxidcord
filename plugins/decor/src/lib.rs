use std::collections::{HashMap, HashSet, VecDeque};

use plugin_sdk::model::{DisplayOverrideFull, Presence};
use plugin_sdk::types::FetchRequest;
use plugin_sdk::{Plugin, PluginExt, User, actions, export_plugin, host, log, now_ms, storage, timer};
use serde_json::Value;

const API_URL: &str = "https://decor.fieryflames.dev/api/users";
const CDN_URL: &str = "https://ugc.decor.fieryflames.dev";
const CACHE_KEY: &str = "decorations";
const FETCH_TAG: &str = "fetch";
const FIRST_FETCH_MS: u32 = 1000;
const BATCH_INTERVAL_MS: u32 = 6500;
const RETRY_MS: u32 = 60_000;
const BATCH_SIZE: usize = 40;
const MAX_ENTRIES: usize = 6000;
const MAX_ASSET_BYTES: usize = 64;
const COOLDOWN_MS: i64 = 4 * 60 * 60 * 1000;

struct Entry {
	asset: Option<String>,
	at_ms: i64,
}

#[derive(Default)]
struct Decor {
	entries: HashMap<u64, Entry>,
	queue: VecDeque<u64>,
	queued: HashSet<u64>,
	timer_armed: bool,
}

fn valid_asset(asset: &str) -> Option<String> {
	let valid = !asset.is_empty()
		&& asset.len() <= MAX_ASSET_BYTES
		&& asset
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
	valid.then(|| asset.to_owned())
}

fn ids_query(ids: &[u64]) -> String {
	let quoted: Vec<String> = ids.iter().map(|id| format!("%22{id}%22")).collect();
	format!("%5B{}%5D", quoted.join("%2C"))
}

fn fetch_assets(ids: &[u64]) -> Result<HashMap<u64, String>, String> {
	let request = FetchRequest {
		method: "GET".to_owned(),
		url: format!("{API_URL}?ids={}", ids_query(ids)),
		headers: vec![("accept".to_owned(), "application/json".to_owned())],
		body: None,
	};
	let response = host::fetch(&request).map_err(|error| format!("{error:?}"))?;
	if response.status != 200 {
		return Err(format!("status {}", response.status));
	}
	let Value::Object(answer) = serde_json::from_slice(&response.body).map_err(|error| error.to_string())?
	else {
		return Err("unexpected answer".to_owned());
	};
	Ok(answer
		.into_iter()
		.filter_map(|(id, asset)| Some((id.parse().ok()?, valid_asset(asset.as_str()?)?)))
		.collect())
}

fn decoration_override(asset: &str) -> DisplayOverrideFull {
	DisplayOverrideFull {
		name: None,
		suffix: None,
		color: None,
		badges: Vec::new(),
		avatar: None,
		banner: None,
		decoration: Some(format!("{CDN_URL}/{asset}.png")),
		pronouns: None,
	}
}

impl Decor {
	fn load_cache(&mut self) {
		let Ok(Some(saved)) = storage::get(CACHE_KEY) else {
			return;
		};
		for line in String::from_utf8_lossy(&saved).lines() {
			let mut fields = line.split('\t');
			let (Some(id), Some(at_ms), Some(asset)) = (fields.next(), fields.next(), fields.next()) else {
				continue;
			};
			if let (Ok(id), Ok(at_ms)) = (id.parse(), at_ms.parse()) {
				self.entries.insert(
					id,
					Entry {
						asset: valid_asset(asset),
						at_ms,
					},
				);
			}
		}
	}

	fn save_cache(&mut self) {
		let cutoff = now_ms() - COOLDOWN_MS;
		self.entries.retain(|_, entry| entry.at_ms > cutoff);
		if self.entries.len() > MAX_ENTRIES {
			self.entries.retain(|_, entry| entry.asset.is_some());
		}
		let lines: Vec<String> = self
			.entries
			.iter()
			.map(|(id, entry)| format!("{id}\t{}\t{}", entry.at_ms, entry.asset.as_deref().unwrap_or("")))
			.collect();
		if let Err(error) = storage::set(CACHE_KEY, lines.join("\n").as_bytes()) {
			log::warn!("could not keep the decorations: {error:?}");
		}
	}

	fn enqueue(&mut self, user_id: u64) {
		if self.queued.insert(user_id) {
			self.queue.push_back(user_id);
			self.arm(FIRST_FETCH_MS);
		}
	}

	fn arm(&mut self, delay_ms: u32) {
		if self.timer_armed {
			return;
		}
		match timer::once(delay_ms, FETCH_TAG) {
			Ok(_) => self.timer_armed = true,
			Err(error) => log::warn!("could not schedule the decoration lookup: {error:?}"),
		}
	}

	fn apply(&mut self, batch: &[u64], mut assets: HashMap<u64, String>) {
		let now = now_ms();
		for user_id in batch {
			self.queued.remove(user_id);
			let asset = assets.remove(user_id);
			let changed = self.entries.get(user_id).and_then(|entry| entry.asset.as_ref()) != asset.as_ref();
			self.entries.insert(*user_id, Entry { asset, at_ms: now });
			if changed && let Err(error) = actions::invalidate_user(*user_id) {
				log::debug!("could not refresh user {user_id}: {error:?}");
			}
		}
		self.save_cache();
	}
}

impl Plugin for Decor {
	fn init(&mut self) -> Result<(), String> {
		self.load_cache();
		Ok(())
	}
}

impl PluginExt for Decor {
	fn display_user_full(
		&mut self,
		_guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		if user.bot {
			return None;
		}
		let fresh = self
			.entries
			.get(&user.id)
			.is_some_and(|entry| now_ms() - entry.at_ms < COOLDOWN_MS);
		if !fresh {
			self.enqueue(user.id);
		}
		self.entries
			.get(&user.id)?
			.asset
			.as_deref()
			.map(decoration_override)
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag != FETCH_TAG {
			return;
		}
		self.timer_armed = false;
		let take = self.queue.len().min(BATCH_SIZE);
		let batch: Vec<u64> = self.queue.drain(..take).collect();
		if batch.is_empty() {
			return;
		}
		match fetch_assets(&batch) {
			Ok(assets) => self.apply(&batch, assets),
			Err(reason) => {
				log::warn!("could not look up decorations: {reason}");
				self.queue.extend(batch);
				self.arm(RETRY_MS);
				return;
			}
		}
		if !self.queue.is_empty() {
			self.arm(BATCH_INTERVAL_MS);
		}
	}
}

export_plugin!(Decor, ext);
