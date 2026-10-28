use std::collections::VecDeque;
use std::fmt;

use plugin_sdk::model::{DisplayOverrideFull, Presence};
use plugin_sdk::types::{FetchRequest, SettingChange};
use plugin_sdk::{
	Event, Plugin, PluginExt, User, actions, data, export_plugin, host, log, now_ms, storage, timer,
};
use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};

const INDEX_URL: &str = "https://usrbg.is-hardly.online/users";
const DEFAULT_BASE: &str = "https://usrbg.is-hardly.online/usrbg/v2";
const NITRO_FIRST_KEY: &str = "nitroFirst";
const TABLE_KEY: &str = "index.table";
const META_KEY: &str = "index.meta";
const REFRESH_TAG: &str = "refresh";
const RECORD_BYTES: usize = 12;
const NO_ETAG: u32 = u32::MAX;
const FIRST_FETCH_MS: i64 = 1000;
const RETRY_MS: u32 = 60 * 60 * 1000;
const REFRESH_MS: i64 = 24 * 60 * 60 * 1000;
const RECENT_USERS: usize = 48;

#[derive(Deserialize)]
struct Index {
	#[serde(default)]
	endpoint: String,
	#[serde(default)]
	bucket: String,
	#[serde(default)]
	prefix: String,
	users: Users,
}

struct Users(Vec<(u64, u32)>);

struct UsersVisitor;

impl<'de> Visitor<'de> for UsersVisitor {
	type Value = Users;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a map of user ids to etags")
	}

	fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Users, A::Error> {
		let mut users = Vec::new();
		while let Some((id, etag)) = map.next_entry::<&str, &str>()? {
			if let Ok(id) = id.parse() {
				users.push((id, parse_etag(etag)));
			}
		}
		Ok(Users(users))
	}
}

impl<'de> Deserialize<'de> for Users {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_map(UsersVisitor)
	}
}

fn parse_etag(text: &str) -> u32 {
	u32::from_str_radix(text, 16)
		.ok()
		.filter(|_| text.len() <= 8)
		.unwrap_or(NO_ETAG)
}

impl Index {
	fn base(&self) -> String {
		let parts: Vec<&str> = [
			self.endpoint.trim_end_matches('/'),
			self.bucket.trim_matches('/'),
			self.prefix.trim_matches('/'),
		]
		.into_iter()
		.filter(|part| !part.is_empty())
		.collect();
		if parts.is_empty() {
			DEFAULT_BASE.to_owned()
		} else {
			parts.join("/")
		}
	}
}

fn fetch_index() -> Result<(String, Vec<(u64, u32)>), String> {
	let request = FetchRequest {
		method: "GET".to_owned(),
		url: INDEX_URL.to_owned(),
		headers: vec![("accept".to_owned(), "application/json".to_owned())],
		body: None,
	};
	let response = host::fetch(&request).map_err(|error| format!("{error:?}"))?;
	if response.status != 200 {
		return Err(format!("status {}", response.status));
	}
	let index: Index = serde_json::from_slice(&response.body).map_err(|error| error.to_string())?;
	Ok((index.base(), index.users.0))
}

fn record_id(record: &[u8; RECORD_BYTES]) -> u64 {
	let [a, b, c, d, e, f, g, h, ..] = *record;
	u64::from_be_bytes([a, b, c, d, e, f, g, h])
}

fn record_etag(record: &[u8; RECORD_BYTES]) -> u32 {
	let [.., a, b, c, d] = *record;
	u32::from_be_bytes([a, b, c, d])
}

fn has_nitro_banner(user_id: u64) -> bool {
	data::user_profile(user_id)
		.ok()
		.flatten()
		.is_some_and(|profile| profile.banner.is_some())
}

fn banner_override(url: String) -> DisplayOverrideFull {
	DisplayOverrideFull {
		name: None,
		suffix: None,
		color: None,
		badges: Vec::new(),
		avatar: None,
		banner: Some(url),
		decoration: None,
		pronouns: None,
	}
}

struct Usrbg {
	nitro_first: bool,
	base: String,
	table: Vec<u8>,
	fetched_ms: i64,
	recent: VecDeque<u64>,
}

impl Default for Usrbg {
	fn default() -> Self {
		Self {
			nitro_first: true,
			base: DEFAULT_BASE.to_owned(),
			table: Vec::new(),
			fetched_ms: 0,
			recent: VecDeque::new(),
		}
	}
}

impl Usrbg {
	fn load_cache(&mut self) {
		let (Ok(Some(meta)), Ok(Some(table))) = (storage::get(META_KEY), storage::get(TABLE_KEY)) else {
			return;
		};
		let meta = String::from_utf8_lossy(&meta);
		let Some((fetched, base)) = meta.split_once('\n') else {
			return;
		};
		let Ok(fetched) = fetched.parse() else {
			return;
		};
		if table.len() % RECORD_BYTES == 0 {
			self.fetched_ms = fetched;
			self.base = base.to_owned();
			self.table = table;
		}
	}

	fn etag_of(&self, user_id: u64) -> Option<u32> {
		let (records, _) = self.table.as_chunks::<RECORD_BYTES>();
		let index = records.binary_search_by_key(&user_id, record_id).ok()?;
		Some(record_etag(&records[index]))
	}

	fn banner_url(&self, user_id: u64, etag: u32) -> String {
		if etag == NO_ETAG {
			format!("{}/{user_id}", self.base)
		} else {
			format!("{}/{user_id}?{etag:04x}", self.base)
		}
	}

	fn remember(&mut self, user_id: u64) {
		if self.recent.contains(&user_id) {
			return;
		}
		if self.recent.len() == RECENT_USERS {
			self.recent.pop_front();
		}
		self.recent.push_back(user_id);
	}

	fn refresh(&mut self) {
		match fetch_index() {
			Ok((base, users)) => self.store(base, users),
			Err(reason) => {
				log::warn!("could not download the USRBG list: {reason}");
				schedule(REFRESH_TAG, RETRY_MS);
			}
		}
	}

	fn store(&mut self, base: String, mut users: Vec<(u64, u32)>) {
		users.sort_unstable_by_key(|(id, _)| *id);
		users.dedup_by_key(|(id, _)| *id);
		if users.is_empty() {
			log::warn!("the USRBG list was empty");
			schedule(REFRESH_TAG, RETRY_MS);
			return;
		}
		let mut table = Vec::with_capacity(users.len() * RECORD_BYTES);
		for (id, etag) in users {
			table.extend_from_slice(&id.to_be_bytes());
			table.extend_from_slice(&etag.to_be_bytes());
		}
		let now = now_ms();
		let saved = storage::set(TABLE_KEY, &table)
			.and_then(|()| storage::set(META_KEY, format!("{now}\n{base}").as_bytes()));
		if let Err(error) = saved {
			log::warn!("could not keep the USRBG list: {error:?}");
		}
		self.table = table;
		self.base = base;
		self.fetched_ms = now;
		schedule(REFRESH_TAG, REFRESH_MS as u32);
		for user_id in &self.recent {
			if self.etag_of(*user_id).is_some()
				&& let Err(error) = actions::invalidate_user(*user_id)
			{
				log::debug!("could not refresh user {user_id}: {error:?}");
			}
		}
	}
}

fn schedule(tag: &str, delay_ms: u32) {
	if let Err(error) = timer::once(delay_ms, tag) {
		log::warn!("could not schedule {tag}: {error:?}");
	}
}

impl Plugin for Usrbg {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{NITRO_FIRST_KEY}")) {
			self.nitro_first = !matches!(String::from_utf8_lossy(&saved).trim(), "false" | "0" | "off");
		}
		self.load_cache();
		let remaining = (REFRESH_MS - (now_ms() - self.fetched_ms)).clamp(FIRST_FETCH_MS, REFRESH_MS);
		schedule(REFRESH_TAG, remaining as u32);
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == NITRO_FIRST_KEY
		{
			self.nitro_first = !matches!(value.trim(), "false" | "0" | "off");
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"nitroFirst":{"type":"boolean","title":"Nitro banner first","description":"Use the Nitro banner when a user has both a Nitro and a USRBG banner","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for Usrbg {
	fn display_user_full(
		&mut self,
		_guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		self.remember(user.id);
		let etag = self.etag_of(user.id)?;
		if self.nitro_first && has_nitro_banner(user.id) {
			return None;
		}
		Some(banner_override(self.banner_url(user.id, etag)))
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == REFRESH_TAG {
			self.refresh();
		}
	}
}

export_plugin!(Usrbg, ext);
