use plugin_sdk::types::SettingChange;
use plugin_sdk::ui::{self, button, column, input, row, text};
use plugin_sdk::{Event, MessageView, Plugin, Ui, export_plugin, host, log, storage};

const TERMS_KEY: &str = "terms";
const WHOLE_WORD_KEY: &str = "whole_word";
const WHOLE_WORD_STORAGE_KEY: &str = "settings.whole_word";
const PAGE_SLOT: &str = "settings-page";
const ADD_ACTION: &str = "add-term";
const REMOVE_PREFIX: &str = "remove:";
const CLEAR_ACTION: &str = "clear-terms";
const MAX_TERMS: usize = 200;
const MAX_TERM_CHARS: usize = 64;

struct WordMute {
	terms: Vec<String>,
	whole_word: bool,
	own_id: Option<u64>,
}

impl Default for WordMute {
	fn default() -> Self {
		Self {
			terms: Vec::new(),
			whole_word: true,
			own_id: None,
		}
	}
}

impl WordMute {
	fn add(&mut self, raw: &str) -> Result<(), String> {
		let term = normalize(raw);
		if term.is_empty() {
			return Err("type a word or phrase to mute".to_owned());
		}
		if term.chars().count() > MAX_TERM_CHARS {
			return Err(format!("terms can be at most {MAX_TERM_CHARS} characters"));
		}
		if self.terms.contains(&term) {
			return Ok(());
		}
		if self.terms.len() >= MAX_TERMS {
			return Err(format!("you can mute at most {MAX_TERMS} terms"));
		}
		self.terms.push(term);
		self.save();
		Ok(())
	}

	fn remove(&mut self, raw: &str) -> Result<(), String> {
		let term = normalize(raw);
		let before = self.terms.len();
		self.terms.retain(|saved| *saved != term);
		if self.terms.len() == before {
			return Err(format!("{term} is not muted"));
		}
		self.save();
		Ok(())
	}

	fn clear(&mut self) {
		self.terms.clear();
		self.save();
	}

	fn save(&self) {
		if let Err(error) = storage::set(TERMS_KEY, self.terms.join("\n").as_bytes()) {
			log::warn!("could not save muted terms: {error:?}");
		}
	}

	fn is_own(&mut self, author_id: u64) -> bool {
		if self.own_id.is_none() {
			self.own_id = host::current_user().ok().map(|me| me.id);
		}
		self.own_id == Some(author_id)
	}

	fn muted(&self, content: &str) -> bool {
		let lowered = content.to_lowercase();
		self.terms
			.iter()
			.any(|term| contains_term(&lowered, term, self.whole_word))
	}

	fn show_page(&self) {
		let mut list = column().gap(8).padding(8);
		list = list.child(text(format!("{} of {MAX_TERMS} terms muted", self.terms.len())).muted());
		for term in &self.terms {
			list = list.child(
				row().gap(8).child(text(term.as_str())).child(
					button(format!("{REMOVE_PREFIX}{term}"), "Remove")
						.icon("trash")
						.variant("danger"),
				),
			);
		}
		list = list.child(input(ADD_ACTION, "word or phrase, Enter to mute"));
		if !self.terms.is_empty() {
			list = list.child(button(CLEAR_ACTION, "Clear all").variant("danger"));
		}
		let tree = [Ui::from(list)];
		if let Err(error) = ui::register_slot(PAGE_SLOT, &tree) {
			log::warn!("could not update the {PAGE_SLOT} slot: {error:?}");
		}
	}
}

impl Plugin for WordMute {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(TERMS_KEY) {
			self.terms = String::from_utf8_lossy(&saved)
				.lines()
				.map(normalize)
				.filter(|term| !term.is_empty())
				.take(MAX_TERMS)
				.collect();
		}
		if let Ok(Some(saved)) = storage::get(WHOLE_WORD_STORAGE_KEY) {
			self.whole_word = parse_flag(&String::from_utf8_lossy(&saved));
		}
		for (name, description) in [
			("mute", "Hide messages containing a word or phrase: /mute <text>"),
			("unmute", "Show a muted word or phrase again: /unmute <text>"),
			("mutes", "Open the list of muted words"),
			("unmuteall", "Clear every muted word"),
		] {
			if let Err(error) = ui::register_command(name, description) {
				log::warn!("could not register /{name}: {error:?}");
			}
		}
		self.show_page();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == WHOLE_WORD_KEY
		{
			self.whole_word = parse_flag(&value);
		}
	}

	fn filter_message(&mut self, msg: &MessageView) -> bool {
		if self.terms.is_empty() || msg.content.is_empty() || self.is_own(msg.author.id) {
			return true;
		}
		!self.muted(&msg.content)
	}

	fn on_ui_action(&mut self, id: &str, payload: &str) -> Vec<Ui> {
		let outcome = if id == ADD_ACTION {
			self.add(payload)
		} else if id == CLEAR_ACTION {
			self.clear();
			Ok(())
		} else if let Some(term) = id.strip_prefix(REMOVE_PREFIX) {
			self.remove(term)
		} else {
			Ok(())
		};
		if let Err(message) = outcome {
			log::debug!("word mute: {message}");
		}
		self.show_page();
		Vec::new()
	}

	fn on_command(&mut self, name: &str, args: &str, _channel_id: u64) -> Result<(), String> {
		let outcome = match name {
			"mute" => self.add(args),
			"unmute" => self.remove(args),
			"unmuteall" => {
				self.clear();
				Ok(())
			}
			_ => Ok(()),
		};
		self.show_page();
		outcome
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"whole_word":{"type":"boolean","title":"Match whole words only","default":true}}}"#
			.to_owned()
	}
}

fn normalize(raw: &str) -> String {
	raw.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
		.to_lowercase()
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

fn contains_term(haystack: &str, term: &str, whole_word: bool) -> bool {
	if !whole_word {
		return haystack.contains(term);
	}
	haystack.match_indices(term).any(|(start, found)| {
		let before = haystack[..start].chars().next_back();
		let after = haystack[start + found.len()..].chars().next();
		!before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
	})
}

export_plugin!(WordMute);
