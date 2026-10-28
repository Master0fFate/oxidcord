use std::collections::{HashMap, HashSet, VecDeque};

use plugin_sdk::model::{
	ButtonVariant, Color as ApiColor, EmbedCard, MessageFull, Space, ViewEvent, ViewEventKind,
};
use plugin_sdk::patch::Patch;
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, HostError, Md, MessageRef, Plugin, PluginExt, Reply, actions, client, data, export_plugin, host,
	log, storage, timer,
};

const ACCESSORY_POINT: &str = "message-accessory";
const JUMP_PREFIX: &str = "jump:";
const LOAD_TAG: &str = "load";
const LOAD_DELAY_MS: u32 = 400;
const RETRY_DELAY_MS: u32 = 2_000;
const LOADS_PER_TICK: usize = 2;
const MAX_LINKS: usize = 5;
const MAX_PENDING: usize = 16;
const MAX_WAITING: usize = 8;
const MAX_LINKED: usize = 128;
const MAX_ATTEMPTED: usize = 1024;
const CONTENT_CHARS: usize = 500;
const SETTING_KEYS: [&str; 4] = ["messageBackgroundColor", "automodEmbeds", "listMode", "idList"];

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Automod {
	Always,
	Prefer,
	#[default]
	Never,
}

#[derive(Default)]
struct Config {
	background: bool,
	automod: Automod,
	whitelist: bool,
	listed: Vec<String>,
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		match key {
			"messageBackgroundColor" => self.background = value == "true",
			"automodEmbeds" => {
				self.automod = match value {
					"always" => Automod::Always,
					"prefer" => Automod::Prefer,
					_ => Automod::Never,
				}
			}
			"listMode" => self.whitelist = value == "whitelist",
			"idList" => {
				self.listed = value
					.split(|character: char| character == ',' || character.is_whitespace())
					.filter(|id| !id.is_empty())
					.map(str::to_owned)
					.collect();
			}
			_ => {}
		}
	}

	fn allows(&self, ids: [u64; 3]) -> bool {
		let listed = ids
			.iter()
			.any(|id| self.listed.iter().any(|entry| *entry == id.to_string()));
		listed == self.whitelist
	}
}

struct Linked {
	author: String,
	content: String,
	image: Option<String>,
	rich: bool,
	channel_id: u64,
	guild_id: Option<u64>,
}

struct Pending {
	target: MessageRef,
	messages: Vec<MessageRef>,
}

#[derive(Default)]
struct MessageLinkEmbeds {
	config: Config,
	linked: HashMap<u64, Linked>,
	order: VecDeque<u64>,
	pending: VecDeque<Pending>,
	attempted: HashSet<u64>,
	armed: bool,
}

fn digits(text: &str) -> usize {
	text.bytes().take_while(u8::is_ascii_digit).count()
}

fn is_discord_host(host: &str) -> bool {
	["discord.com", "discordapp.com"].iter().any(|base| {
		host.strip_suffix(base).is_some_and(|prefix| {
			prefix.is_empty()
				|| prefix.strip_suffix('.').is_some_and(|label| {
					!label.is_empty()
						&& label
							.bytes()
							.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
				})
		})
	})
}

fn parse_link(content: &str, at: usize) -> Option<(u64, u64)> {
	let before = &content[..at];
	let start = before.rfind("http")?;
	if content[..start].ends_with('<') {
		return None;
	}
	let host = before[start..]
		.strip_prefix("https://")
		.or_else(|| before[start..].strip_prefix("http://"))?;
	if !is_discord_host(host) {
		return None;
	}
	let rest = &content[at + "/channels/".len()..];
	let guild_length = if rest.starts_with("@me") { 3 } else { digits(rest) };
	if guild_length == 0 || (guild_length != 3 && !(17..=20).contains(&guild_length)) {
		return None;
	}
	let rest = rest[guild_length..].strip_prefix('/')?;
	let channel_length = digits(rest);
	if !(17..=20).contains(&channel_length) {
		return None;
	}
	let channel = rest[..channel_length].parse().ok()?;
	let rest = rest[channel_length..].strip_prefix('/')?;
	let message_length = digits(rest).min(20);
	if message_length < 17 {
		return None;
	}
	Some((channel, rest[..message_length].parse().ok()?))
}

fn message_links(content: &str) -> Vec<(u64, u64)> {
	let mut found: Vec<(u64, u64)> = Vec::new();
	for (at, _) in content.match_indices("/channels/") {
		if found.len() == MAX_LINKS {
			break;
		}
		if let Some((channel, message)) = parse_link(content, at)
			&& !found.iter().any(|(_, seen)| *seen == message)
		{
			found.push((channel, message));
		}
	}
	found
}

fn is_image(content_type: Option<&str>) -> bool {
	content_type.is_some_and(|kind| kind.starts_with("image/"))
}

fn linked_of(full: &MessageFull) -> Linked {
	let images = full
		.attachments
		.iter()
		.filter(|attachment| is_image(attachment.content_type.as_deref()))
		.count();
	let other = full.attachments.len() - images;
	let content = if full.view.content.is_empty() && full.attachments.len() > images {
		format!("[no content, {} attachments]", full.attachments.len())
	} else {
		full.view.content.chars().take(CONTENT_CHARS).collect()
	};
	Linked {
		author: full.view.author.display_name.clone(),
		content,
		image: full
			.attachments
			.iter()
			.find(|attachment| is_image(attachment.content_type.as_deref()))
			.map(|attachment| attachment.url.clone()),
		rich: other > 0 || !full.embeds.is_empty(),
		channel_id: full.view.channel_id,
		guild_id: full.view.guild_id,
	}
}

fn channel_info(channel_id: u64) -> Option<(String, Option<u64>)> {
	let detail = data::channel_detail(channel_id).ok().flatten()?;
	let label = match detail.guild_id {
		Some(_) => format!("Server - #{}", detail.name),
		None if detail.recipients.len() > 1 => "Group DM".to_owned(),
		None => "Direct Message".to_owned(),
	};
	Some((label, detail.guild_id))
}

impl MessageLinkEmbeds {
	fn remember(&mut self, message_id: u64, linked: Linked) {
		if self.linked.insert(message_id, linked).is_none() {
			self.order.push_back(message_id);
			if self.order.len() > MAX_LINKED
				&& let Some(oldest) = self.order.pop_front()
			{
				self.linked.remove(&oldest);
			}
		}
	}

	fn settle(&mut self, message_id: u64) {
		if self.attempted.len() >= MAX_ATTEMPTED {
			self.attempted.clear();
		}
		self.attempted.insert(message_id);
	}

	fn arm(&mut self, delay_ms: u32) {
		if self.armed {
			return;
		}
		match timer::once(delay_ms, LOAD_TAG) {
			Ok(_) => self.armed = true,
			Err(error) => log::debug!("could not schedule the message load: {error:?}"),
		}
	}

	fn enqueue(&mut self, target: MessageRef, message: MessageRef) {
		if let Some(entry) = self.pending.iter_mut().find(|entry| entry.target.id == target.id) {
			if entry.messages.len() < MAX_WAITING && !entry.messages.contains(&message) {
				entry.messages.push(message);
			}
			return;
		}
		if self.pending.len() >= MAX_PENDING {
			return;
		}
		self.pending.push_back(Pending {
			target,
			messages: vec![message],
		});
		self.arm(LOAD_DELAY_MS);
	}

	fn resolve(&mut self, channel_id: u64, message_id: u64, source: &MessageFull) -> bool {
		if self.linked.contains_key(&message_id) {
			return true;
		}
		let found = client::get_message(channel_id, message_id)
			.ok()
			.flatten()
			.or_else(|| data::fetch_message(channel_id, message_id).ok().flatten());
		match found {
			Some(full) => {
				self.remember(message_id, linked_of(&full));
				true
			}
			None => {
				if !self.attempted.contains(&message_id) {
					let target = MessageRef {
						id: message_id,
						channel_id,
						guild_id: None,
					};
					let message = MessageRef {
						id: source.view.id,
						channel_id: source.view.channel_id,
						guild_id: source.view.guild_id,
					};
					self.enqueue(target, message);
				}
				false
			}
		}
	}

	fn card(&self, linked: &Linked, label: String) -> EmbedCard {
		let accent = self
			.config
			.background
			.then(|| ApiColor::Token("background-tertiary".to_owned()));
		let compact = match self.config.automod {
			Automod::Always => true,
			Automod::Prefer => !linked.rich,
			Automod::Never => false,
		};
		if compact {
			return EmbedCard {
				title: None,
				description: Some(format!("**{}** {}", linked.author, linked.content)),
				url: None,
				accent,
				image: linked.image.clone(),
				footer: Some(label),
			};
		}
		let mut card = view::card(linked.author.clone());
		card.description = (!linked.content.is_empty()).then(|| linked.content.clone());
		card.accent = accent;
		card.image = linked.image.clone();
		card.footer = Some(label);
		card
	}

	fn entry(&self, message_id: u64, label: String) -> Option<Node> {
		let linked = self.linked.get(&message_id)?;
		Some(
			view::column()
				.gap(Space::Xs)
				.child(view::embed(self.card(linked, label)))
				.child(
					view::button(format!("{JUMP_PREFIX}{message_id}"), "Jump")
						.icon("arrow-up-right")
						.variant(ButtonVariant::Link),
				),
		)
	}
}

impl Plugin for MessageLinkEmbeds {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"messageBackgroundColor":{"type":"boolean","title":"Message background color","description":"Background color for messages in rich embeds","default":false},"automodEmbeds":{"type":"string","title":"Automod embeds","description":"Use compact embeds instead of rich embeds: always, prefer (rich only when the message has files or embeds) or never","enum":["always","prefer","never"],"default":"never"},"listMode":{"type":"string","title":"List mode","description":"Whether to use the ID list as blacklist or whitelist","enum":["blacklist","whitelist"],"default":"blacklist"},"idList":{"type":"string","title":"ID list","description":"Guild, channel or user IDs to blacklist or whitelist, separated by commas","default":""}}}"#
			.to_owned()
	}
}

impl PluginExt for MessageLinkEmbeds {
	fn intercept_message(&mut self, msg: &MessageFull, _content: Vec<Md>) -> Patch {
		let links = message_links(&msg.view.content);
		if links.is_empty() {
			return Patch::keep();
		}
		let mut entries: Vec<Node> = Vec::with_capacity(links.len());
		for (channel_id, message_id) in links {
			let Some((label, guild_id)) = channel_info(channel_id) else {
				continue;
			};
			if !self
				.config
				.allows([guild_id.unwrap_or_default(), channel_id, msg.view.author.id])
			{
				continue;
			}
			if self.resolve(channel_id, message_id, msg)
				&& let Some(entry) = self.entry(message_id, label)
			{
				entries.push(entry);
			}
		}
		if entries.is_empty() {
			Patch::keep()
		} else {
			Patch::keep().accessory(entries)
		}
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag != LOAD_TAG {
			return;
		}
		self.armed = false;
		let mut delay = LOAD_DELAY_MS;
		for _ in 0..LOADS_PER_TICK {
			let Some(entry) = self.pending.pop_front() else {
				break;
			};
			let target = entry.target;
			match data::fetch_message(target.channel_id, target.id) {
				Ok(found) => {
					self.settle(target.id);
					if let Some(full) = found {
						self.remember(target.id, linked_of(&full));
						for message in entry.messages {
							if let Err(error) = actions::invalidate_message(message) {
								log::debug!("could not refresh message {}: {error:?}", message.id);
							}
						}
					}
				}
				Err(HostError::RateLimited) => {
					self.pending.push_front(entry);
					delay = RETRY_DELAY_MS;
					break;
				}
				Err(error) => {
					log::debug!("could not load message {}: {error:?}", target.id);
					self.settle(target.id);
				}
			}
		}
		if !self.pending.is_empty() {
			self.arm(delay);
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click || event.point != ACCESSORY_POINT {
			return Reply::none();
		}
		let Some(message_id) = event
			.node
			.strip_prefix(JUMP_PREFIX)
			.and_then(|id| id.parse::<u64>().ok())
		else {
			return Reply::none();
		};
		let Some(linked) = self.linked.get(&message_id) else {
			return Reply::none();
		};
		let target = MessageRef {
			id: message_id,
			channel_id: linked.channel_id,
			guild_id: linked.guild_id,
		};
		if let Err(error) = host::open_channel(target.guild_id, target.channel_id)
			.and_then(|()| actions::jump_to_message(target))
		{
			log::debug!("could not jump to message {message_id}: {error:?}");
			return Reply::none().toast("Could not open that message");
		}
		Reply::none()
	}
}

export_plugin!(MessageLinkEmbeds, ext);
