use std::collections::{HashMap, HashSet, VecDeque};

use plugin_sdk::model::{
	ButtonVariant, Color as ApiColor, EmbedAction, EmbedCard, EmbedInfo, MessageFull, ViewEvent,
	ViewEventKind,
};
use plugin_sdk::patch::Patch;
use plugin_sdk::types::{FetchRequest, SettingChange};
use plugin_sdk::view;
use plugin_sdk::{
	Event, HostError, Md, MessageRef, Plugin, PluginExt, Reply, actions, export_plugin, host, log, now_ms,
	storage, timer,
};
use serde_json::{Map, Value, json};

const SETTING_KEYS: [&str; 3] = ["hideButton", "replaceElements", "dearrowByDefault"];
const CACHE_KEY: &str = "cache";
const ACCESSORY_POINT: &str = "message-accessory";
const TOGGLE_BUTTON: &str = "toggle";
const FETCH_TAG: &str = "fetch";
const BRANDING_URL: &str = "https://sponsor.ajay.app/api/branding?videoID=";
const THUMBNAIL_URL: &str = "https://dearrow-thumb.ajay.app/api/v1/getThumbnail?videoID=";
const FETCH_DELAY_MS: u32 = 500;
const RETRY_DELAY_MS: u32 = 6_000;
const FETCHES_PER_TICK: usize = 2;
const TTL_MS: i64 = 24 * 60 * 60 * 1000;
const FAILURE_TTL_MS: i64 = 10 * 60 * 1000;
const MAX_CACHED: usize = 300;
const MAX_PENDING: usize = 32;
const MAX_WAITING: usize = 8;
const MAX_FLIPPED: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Replace {
	#[default]
	Both,
	Titles,
	Thumbnails,
}

struct Config {
	hide_button: bool,
	replace: Replace,
	by_default: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			hide_button: false,
			replace: Replace::Both,
			by_default: true,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		match key {
			"hideButton" => self.hide_button = value == "true",
			"replaceElements" => {
				self.replace = match value {
					"title" => Replace::Titles,
					"thumbnail" => Replace::Thumbnails,
					_ => Replace::Both,
				}
			}
			"dearrowByDefault" => self.by_default = value != "false",
			_ => {}
		}
	}
}

struct Entry {
	fetched_ms: i64,
	title: Option<String>,
	time: Option<f64>,
	failed: bool,
}

impl Entry {
	fn fresh(&self, now: i64) -> bool {
		now - self.fetched_ms < if self.failed { FAILURE_TTL_MS } else { TTL_MS }
	}
}

struct Wanted {
	video: String,
	messages: Vec<MessageRef>,
}

#[derive(Default)]
struct Dearrow {
	config: Config,
	entries: HashMap<String, Entry>,
	pending: VecDeque<Wanted>,
	flipped: HashSet<u64>,
	armed: bool,
}

fn is_video_id(text: &str) -> bool {
	text.len() == 11
		&& text
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn video_id(url: &str) -> Option<&str> {
	let rest = url
		.strip_prefix("https://")
		.or_else(|| url.strip_prefix("http://"))?;
	let (host, path) = rest.split_once('/')?;
	let host = host
		.strip_prefix("www.")
		.or_else(|| host.strip_prefix("m."))
		.unwrap_or(host);
	let candidate = match host {
		"youtu.be" => path.split(['?', '#', '/']).next()?,
		"youtube.com" => {
			if let Some(query) = path.strip_prefix("watch?") {
				query.split(['&', '#']).find_map(|pair| pair.strip_prefix("v="))?
			} else {
				path.strip_prefix("shorts/")
					.or_else(|| path.strip_prefix("embed/"))
					.or_else(|| path.strip_prefix("live/"))?
					.split(['?', '#', '/'])
					.next()?
			}
		}
		_ => return None,
	};
	is_video_id(candidate).then_some(candidate)
}

fn clean_title(title: &str) -> String {
	let mut cleaned = String::with_capacity(title.len());
	let mut previous_space = true;
	let mut characters = title.chars().peekable();
	while let Some(character) = characters.next() {
		let marker =
			character == '>' && previous_space && characters.peek().is_some_and(|next| !next.is_whitespace());
		if !marker {
			cleaned.push(character);
		}
		previous_space = character.is_whitespace() || (marker && previous_space);
	}
	cleaned
}

fn parse_branding(body: &[u8], now: i64) -> Option<Entry> {
	let branding: Value = serde_json::from_slice(body).ok()?;
	let title = branding
		.get("titles")
		.and_then(|titles| titles.get(0))
		.filter(|first| {
			first
				.get("votes")
				.and_then(Value::as_i64)
				.is_some_and(|votes| votes >= 0)
		})
		.and_then(|first| first.get("title")?.as_str())
		.map(clean_title)
		.filter(|title| !title.trim().is_empty());
	let time = branding
		.get("thumbnails")
		.and_then(|thumbnails| thumbnails.get(0))
		.filter(|first| {
			first
				.get("votes")
				.and_then(Value::as_i64)
				.is_some_and(|votes| votes >= 0)
				&& first.get("original").and_then(Value::as_bool) != Some(true)
		})
		.and_then(|first| first.get("timestamp")?.as_f64());
	Some(Entry {
		fetched_ms: now,
		title,
		time,
		failed: false,
	})
}

fn load_cache() -> HashMap<String, Entry> {
	let Some(Value::Object(saved)) = storage::get(CACHE_KEY)
		.ok()
		.flatten()
		.and_then(|bytes| serde_json::from_slice(&bytes).ok())
	else {
		return HashMap::new();
	};
	saved
		.into_iter()
		.filter_map(|(video, entry)| {
			Some((
				video,
				Entry {
					fetched_ms: entry.get("t")?.as_i64()?,
					title: entry.get("title").and_then(Value::as_str).map(str::to_owned),
					time: entry.get("time").and_then(Value::as_f64),
					failed: false,
				},
			))
		})
		.collect()
}

fn message_of(msg: &MessageFull) -> MessageRef {
	MessageRef {
		id: msg.view.id,
		channel_id: msg.view.channel_id,
		guild_id: msg.view.guild_id,
	}
}

impl Dearrow {
	fn replaces_titles(&self) -> bool {
		self.config.replace != Replace::Thumbnails
	}

	fn replaces_thumbnails(&self) -> bool {
		self.config.replace != Replace::Titles
	}

	fn useful(&self, entry: &Entry) -> bool {
		(self.replaces_titles() && entry.title.is_some())
			|| (self.replaces_thumbnails() && entry.time.is_some())
	}

	fn card(&self, embed: &EmbedInfo, video: &str, entry: &Entry) -> EmbedCard {
		let title = entry
			.title
			.clone()
			.filter(|_| self.replaces_titles())
			.or_else(|| embed.title.clone());
		let image = match entry.time {
			Some(time) if self.replaces_thumbnails() => Some(format!("{THUMBNAIL_URL}{video}&time={time}")),
			_ => embed.image.clone().or_else(|| embed.thumbnail.clone()),
		};
		EmbedCard {
			title,
			description: embed.description.clone(),
			url: embed.url.clone(),
			accent: embed.color.map(|color| ApiColor::Rgba(color << 8 | 0xff)),
			image,
			footer: embed.provider.clone(),
		}
	}

	fn arm(&mut self, delay_ms: u32) {
		if self.armed {
			return;
		}
		match timer::once(delay_ms, FETCH_TAG) {
			Ok(_) => self.armed = true,
			Err(error) => log::debug!("could not schedule the dearrow lookup: {error:?}"),
		}
	}

	fn enqueue(&mut self, video: &str, message: MessageRef) {
		if let Some(wanted) = self.pending.iter_mut().find(|wanted| wanted.video == video) {
			if wanted.messages.len() < MAX_WAITING && !wanted.messages.contains(&message) {
				wanted.messages.push(message);
			}
			return;
		}
		if self.pending.len() >= MAX_PENDING {
			return;
		}
		self.pending.push_back(Wanted {
			video: video.to_owned(),
			messages: vec![message],
		});
		self.arm(FETCH_DELAY_MS);
	}

	fn lookup(&self, video: &str) -> Result<Entry, HostError> {
		let response = host::fetch(&FetchRequest {
			method: "GET".to_owned(),
			url: format!("{BRANDING_URL}{video}"),
			headers: Vec::new(),
			body: None,
		})?;
		let now = now_ms();
		match response.status {
			200 => Ok(parse_branding(&response.body, now).unwrap_or(Entry {
				fetched_ms: now,
				title: None,
				time: None,
				failed: true,
			})),
			404 => Ok(Entry {
				fetched_ms: now,
				title: None,
				time: None,
				failed: false,
			}),
			status => {
				log::debug!("dearrow answered {status} for {video}");
				Ok(Entry {
					fetched_ms: now,
					title: None,
					time: None,
					failed: true,
				})
			}
		}
	}

	fn store(&mut self, video: String, entry: Entry) {
		self.entries.insert(video, entry);
		if self.entries.len() > MAX_CACHED
			&& let Some(oldest) = self
				.entries
				.iter()
				.min_by_key(|(_, entry)| entry.fetched_ms)
				.map(|(video, _)| video.clone())
		{
			self.entries.remove(&oldest);
		}
		self.save();
	}

	fn save(&self) {
		let saved: Map<String, Value> = self
			.entries
			.iter()
			.filter(|(_, entry)| !entry.failed)
			.map(|(video, entry)| {
				(
					video.clone(),
					json!({ "t": entry.fetched_ms, "title": entry.title, "time": entry.time }),
				)
			})
			.collect();
		if let Err(error) = storage::set(CACHE_KEY, Value::Object(saved).to_string().as_bytes()) {
			log::warn!("could not save the dearrow cache: {error:?}");
		}
	}
}

impl Plugin for Dearrow {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		self.entries = load_cache();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"hideButton":{"type":"boolean","title":"Hide button","description":"Hides the Dearrow button from YouTube embeds","default":false},"replaceElements":{"type":"string","title":"Replace elements","description":"Choose which elements of the embed will be replaced: both, title or thumbnail","enum":["both","title","thumbnail"],"default":"both"},"dearrowByDefault":{"type":"boolean","title":"Dearrow by default","description":"Dearrow videos automatically","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for Dearrow {
	fn intercept_message(&mut self, msg: &MessageFull, _content: Vec<Md>) -> Patch {
		let now = now_ms();
		let show = self.config.by_default != self.flipped.contains(&msg.view.id);
		let mut actions: Vec<EmbedAction> = Vec::with_capacity(msg.embeds.len());
		let mut any = false;
		for embed in &msg.embeds {
			let video = (embed.provider.as_deref() == Some("YouTube"))
				.then(|| embed.url.as_deref().and_then(video_id))
				.flatten();
			let action = match video {
				None => EmbedAction::Keep,
				Some(video) => match self.entries.get(video) {
					Some(entry) if entry.fresh(now) => {
						if self.useful(entry) {
							any = true;
							if show {
								EmbedAction::Replace(self.card(embed, video, entry))
							} else {
								EmbedAction::Keep
							}
						} else {
							EmbedAction::Keep
						}
					}
					_ => {
						self.enqueue(video, message_of(msg));
						EmbedAction::Keep
					}
				},
			};
			actions.push(action);
		}
		if !any {
			return Patch::keep();
		}
		let patch = Patch::keep().embeds(actions);
		if self.config.hide_button {
			return patch;
		}
		let (label, icon) = if show {
			("Show original", "rotate-ccw")
		} else {
			("Use Dearrow", "sparkles")
		};
		patch.accessory([view::button(TOGGLE_BUTTON, label)
			.icon(icon)
			.variant(ButtonVariant::Link)])
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag != FETCH_TAG {
			return;
		}
		self.armed = false;
		let mut delay = FETCH_DELAY_MS;
		for _ in 0..FETCHES_PER_TICK {
			let Some(wanted) = self.pending.pop_front() else {
				break;
			};
			match self.lookup(&wanted.video) {
				Ok(entry) => {
					self.store(wanted.video, entry);
					for message in wanted.messages {
						if let Err(error) = actions::invalidate_message(message) {
							log::debug!("could not refresh message {}: {error:?}", message.id);
						}
					}
				}
				Err(HostError::RateLimited) => {
					self.pending.push_front(wanted);
					delay = RETRY_DELAY_MS;
					break;
				}
				Err(error) => {
					log::debug!("could not reach dearrow: {error:?}");
					let failed = Entry {
						fetched_ms: now_ms(),
						title: None,
						time: None,
						failed: true,
					};
					self.entries.insert(wanted.video, failed);
				}
			}
		}
		if !self.pending.is_empty() {
			self.arm(delay);
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click || event.point != ACCESSORY_POINT || event.node != TOGGLE_BUTTON
		{
			return Reply::none();
		}
		let (Ok(message_id), Some(channel_id)) = (event.mount.parse::<u64>(), event.context.channel_id)
		else {
			return Reply::none();
		};
		if !self.flipped.remove(&message_id) {
			if self.flipped.len() >= MAX_FLIPPED {
				self.flipped.clear();
			}
			self.flipped.insert(message_id);
		}
		let message = MessageRef {
			id: message_id,
			channel_id,
			guild_id: event.context.guild_id,
		};
		if let Err(error) = actions::invalidate_message(message) {
			log::debug!("could not refresh message {message_id}: {error:?}");
		}
		Reply::none()
	}
}

export_plugin!(Dearrow, ext);
