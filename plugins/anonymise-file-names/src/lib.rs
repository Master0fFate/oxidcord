use std::hash::{BuildHasher, RandomState};

use plugin_sdk::model::{OutgoingAttachment, OutgoingFull, SendDecisionFull, Tone, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Event, Plugin, PluginExt, Reply, export_plugin, log, mount, now_ms, storage};

const BY_DEFAULT_KEY: &str = "anonymiseByDefault";
const METHOD_KEY: &str = "method";
const LENGTH_KEY: &str = "randomisedLength";
const CONSISTENT_KEY: &str = "consistent";
const SETTING_KEYS: [&str; 4] = [BY_DEFAULT_KEY, METHOD_KEY, LENGTH_KEY, CONSISTENT_KEY];
const TOOLBAR_KEY: &str = "anonymise";
const TOGGLE_BUTTON: &str = "toggle";
const ALPHABET: &[u8] = b"0123456789bdfhjkmnpqrstvwxz";
const MAX_LENGTH: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Method {
	Random,
	Consistent,
	Timestamp,
}

impl Method {
	fn parse(raw: &str) -> Self {
		match raw.trim() {
			"consistent" => Self::Consistent,
			"timestamp" => Self::Timestamp,
			_ => Self::Random,
		}
	}
}

struct AnonymiseFileNames {
	by_default: bool,
	method: Method,
	length: usize,
	consistent: String,
	next: bool,
}

impl Default for AnonymiseFileNames {
	fn default() -> Self {
		Self {
			by_default: true,
			method: Method::Random,
			length: 7,
			consistent: "image".to_owned(),
			next: true,
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

fn extension(name: &str) -> &str {
	if let Some(at) = name.rfind(".tar.") {
		let tail = &name[at + 5..];
		if !tail.is_empty()
			&& tail
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
		{
			return &name[at..];
		}
	}
	name.rfind('.').map_or("", |at| &name[at..])
}

fn random_name(length: usize) -> String {
	let state = RandomState::new();
	(0..length)
		.map(|index| ALPHABET[state.hash_one(index) as usize % ALPHABET.len()] as char)
		.collect()
}

impl AnonymiseFileNames {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			BY_DEFAULT_KEY => {
				self.by_default = parse_flag(value);
				self.next = self.by_default;
			}
			METHOD_KEY => self.method = Method::parse(value),
			LENGTH_KEY => {
				if let Ok(length) = value.trim().parse::<usize>() {
					self.length = length.clamp(1, MAX_LENGTH);
				}
			}
			CONSISTENT_KEY => self.consistent = value.trim().to_owned(),
			_ => {}
		}
	}

	fn anonymous_name(&self, original: &str) -> String {
		let extension = extension(original);
		match self.method {
			Method::Random => format!("{}{extension}", random_name(self.length)),
			Method::Consistent => format!("{}{extension}", self.consistent),
			Method::Timestamp => format!("{}{extension}", now_ms()),
		}
	}

	fn toggle_button(&self) -> Node {
		let (tone, tooltip) = if self.next {
			(Tone::Brand, "Using anonymous file names")
		} else {
			(Tone::Muted, "Using normal file names")
		};
		view::icon_button(TOGGLE_BUTTON, "venetian-mask", tooltip).tone(tone)
	}

	fn refresh_button(&self) {
		if let Err(error) = mount::update(mount::COMPOSER_TOOLBAR, TOOLBAR_KEY, [self.toggle_button()]) {
			log::debug!("could not update the toggle: {error:?}");
		}
	}
}

impl Plugin for AnonymiseFileNames {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		let options = mount::Options::new("Anonymise file names");
		if let Err(error) = mount::mount(
			mount::COMPOSER_TOOLBAR,
			TOOLBAR_KEY,
			[self.toggle_button()],
			options,
		) {
			log::warn!("could not add the composer button: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& SETTING_KEYS.contains(&key.as_str())
		{
			self.set(&key, &value);
			self.refresh_button();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"anonymiseByDefault":{"type":"boolean","title":"Anonymise by default","description":"Whether to anonymise file names by default","default":true},"method":{"type":"string","title":"Method","description":"random uses random characters, consistent always the same name, timestamp the current time","enum":["random","consistent","timestamp"],"default":"random"},"randomisedLength":{"type":"integer","title":"Random characters length","description":"Number of random characters in the name","minimum":1,"maximum":64,"default":7},"consistent":{"type":"string","title":"Consistent filename","description":"The name used by the consistent method","default":"image"}}}"#
			.to_owned()
	}
}

impl PluginExt for AnonymiseFileNames {
	fn before_send_full(&mut self, msg: OutgoingFull) -> SendDecisionFull {
		if msg.attachments.is_empty() {
			return SendDecisionFull::Pass;
		}
		let anonymise = std::mem::replace(&mut self.next, self.by_default);
		if anonymise != self.next {
			self.refresh_button();
		}
		if !anonymise {
			return SendDecisionFull::Pass;
		}
		let attachments = msg
			.attachments
			.iter()
			.map(|attachment| OutgoingAttachment {
				name: self.anonymous_name(&attachment.name),
				..attachment.clone()
			})
			.collect();
		SendDecisionFull::Replace(OutgoingFull { attachments, ..msg })
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.point == mount::COMPOSER_TOOLBAR
			&& event.node == TOGGLE_BUTTON
			&& matches!(event.kind, ViewEventKind::Click)
		{
			self.next = !self.next;
			self.refresh_button();
		}
		Reply::none()
	}
}

export_plugin!(AnonymiseFileNames, ext);
