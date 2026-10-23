use plugin_sdk::types::SettingChange;
use plugin_sdk::ui::{self, button, column, input, row, text};
use plugin_sdk::{Event, OutgoingMessage, Plugin, SendDecision, Ui, export_plugin, log, storage};

const SHORTCUT_PREFIX: &str = "shortcut.";
const SETTING_PREFIX: &str = "setting.";
const FACES: &str = "faces";
const FLIP: &str = "flip";
const USER: &str = "user";
const COMMAND: &str = "shortcut";
const TOOLBAR_SLOT: &str = "composer-toolbar";
const POPOVER_SLOT: &str = "popover";
const OPEN_ACTION: &str = "open-shortcuts";
const ADD_ACTION: &str = "add-shortcut";
const DELETE_PREFIX: &str = "delete:";
const USER_DELIMITER: char = ';';
const MAX_SHORTCUTS: usize = 100;
const MAX_NAME_CHARS: usize = 32;
const MAX_REPLACEMENT_CHARS: usize = 500;
const MAX_MESSAGE_CHARS: usize = 2000;
const PREVIEW_CHARS: usize = 40;
const TRAILING_PUNCTUATION: &str = ".,!?)";

const FACE_SHORTCUTS: [(&str, &str); 2] = [("shrug", "¯\\_(ツ)_/¯"), ("lenny", "( ͡° ͜ʖ ͡°)")];
const FLIP_SHORTCUTS: [(&str, &str); 2] = [("tableflip", "(╯°□°)╯︵ ┻━┻"), ("unflip", "┬─┬ ノ( ゜-゜ノ)")];

struct Rules {
	faces: bool,
	flip: bool,
	user: bool,
	shortcuts: Vec<(String, String)>,
}

impl Default for Rules {
	fn default() -> Self {
		Self {
			faces: true,
			flip: true,
			user: true,
			shortcuts: Vec::new(),
		}
	}
}

impl Rules {
	fn lookup(&self, delimiter: char, name: &str) -> Option<&str> {
		if delimiter == USER_DELIMITER {
			if !self.user {
				return None;
			}
			return self
				.shortcuts
				.iter()
				.find(|(known, _)| known == name)
				.map(|(_, replacement)| replacement.as_str());
		}
		[(self.faces, &FACE_SHORTCUTS), (self.flip, &FLIP_SHORTCUTS)]
			.into_iter()
			.filter(|(enabled, _)| *enabled)
			.flat_map(|(_, shortcuts)| shortcuts.iter())
			.find(|(known, _)| *known == name)
			.map(|(_, replacement)| *replacement)
	}

	fn set(&mut self, group: &str, enabled: bool) {
		match group {
			FACES => self.faces = enabled,
			FLIP => self.flip = enabled,
			USER => self.user = enabled,
			_ => {}
		}
	}
}

#[derive(Default)]
struct TextShortcuts {
	rules: Rules,
}

impl Plugin for TextShortcuts {
	fn init(&mut self) -> Result<(), String> {
		self.rules = load_rules();
		if let Err(error) = ui::register_command(
			COMMAND,
			"Manage shortcuts: /shortcut add <name> <text>, remove <name>, list",
		) {
			log::warn!("could not register /{COMMAND}: {error:?}");
		}
		let toolbar = [Ui::from(button(OPEN_ACTION, "Shortcuts").icon("zap"))];
		if let Err(error) = ui::register_slot(TOOLBAR_SLOT, &toolbar) {
			log::warn!("could not register the {TOOLBAR_SLOT} slot: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		let Event::SettingChanged(SettingChange { key, value }) = event else {
			return;
		};
		if ![FACES, FLIP, USER].contains(&key.as_str()) {
			return;
		}
		let enabled = value == "true";
		self.rules.set(&key, enabled);
		if let Err(error) = storage::set(
			&format!("{SETTING_PREFIX}{key}"),
			if enabled { b"1" } else { b"0" },
		) {
			log::warn!("could not save the {key} setting: {error:?}");
		}
	}

	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		let content = expand(&msg.content, &self.rules);
		if content == msg.content || content.trim().is_empty() || content.chars().count() > MAX_MESSAGE_CHARS
		{
			SendDecision::Pass
		} else {
			SendDecision::Replace(OutgoingMessage { content, ..msg })
		}
	}

	fn on_ui_action(&mut self, id: &str, payload: &str) -> Vec<Ui> {
		if id == ADD_ACTION {
			let (name, replacement) = split_first_word(payload);
			if let Err(error) = self.add(name, replacement) {
				log::warn!("shortcut not added: {error}");
			}
		} else if let Some(name) = id.strip_prefix(DELETE_PREFIX) {
			self.remove(name);
		} else if id != OPEN_ACTION {
			return Vec::new();
		}
		self.show_popover();
		Vec::new()
	}

	fn on_command(&mut self, name: &str, args: &str, _channel_id: u64) -> Result<(), String> {
		if name != COMMAND {
			return Ok(());
		}
		let (subcommand, rest) = split_first_word(args);
		match subcommand {
			"add" => {
				let (shortcut, replacement) = split_first_word(rest);
				self.add(shortcut, replacement)
			}
			"remove" => {
				let name = normalize_name(rest);
				if !self.rules.shortcuts.iter().any(|(known, _)| *known == name) {
					return Err(format!("no shortcut named {name}"));
				}
				self.remove(&name);
				Ok(())
			}
			"list" | "" => {
				self.show_popover();
				Ok(())
			}
			_ => Err("use /shortcut add <name> <text>, remove <name> or list".to_owned()),
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"faces":{"type":"boolean","title":"Expand :shrug: and :lenny:","default":true},"flip":{"type":"boolean","title":"Expand :tableflip: and :unflip:","default":true},"user":{"type":"boolean","title":"Expand your own ;shortcuts","default":true}}}"#
			.to_owned()
	}
}

impl TextShortcuts {
	fn add(&mut self, name: &str, replacement: &str) -> Result<(), String> {
		let name = normalize_name(name);
		let replacement = replacement.trim();
		if name.is_empty() || name.len() > MAX_NAME_CHARS || !name.chars().all(is_name_char) {
			return Err(format!(
				"names are 1 to {MAX_NAME_CHARS} characters of a-z 0-9 _ -"
			));
		}
		let length = replacement.chars().count();
		if length == 0 || length > MAX_REPLACEMENT_CHARS {
			return Err(format!(
				"the text must be 1 to {MAX_REPLACEMENT_CHARS} characters"
			));
		}
		let existing = self.rules.shortcuts.iter().position(|(known, _)| *known == name);
		if existing.is_none() && self.rules.shortcuts.len() >= MAX_SHORTCUTS {
			return Err(format!("at most {MAX_SHORTCUTS} shortcuts"));
		}
		storage::set(&shortcut_key(&name), replacement.as_bytes())
			.map_err(|error| format!("could not save: {error:?}"))?;
		match existing {
			Some(index) => self.rules.shortcuts[index].1 = replacement.to_owned(),
			None => {
				self.rules.shortcuts.push((name, replacement.to_owned()));
				self.rules.shortcuts.sort();
			}
		}
		Ok(())
	}

	fn remove(&mut self, name: &str) {
		if let Err(error) = storage::delete(&shortcut_key(name)) {
			log::warn!("could not delete shortcut {name}: {error:?}");
			return;
		}
		self.rules.shortcuts.retain(|(known, _)| known != name);
	}

	fn show_popover(&self) {
		let mut list = column().gap(4).padding(8);
		for (name, replacement) in &self.rules.shortcuts {
			let preview: String = replacement.chars().take(PREVIEW_CHARS).collect();
			list = list.child(
				row()
					.gap(8)
					.child(text(format!("{USER_DELIMITER}{name}")))
					.child(text(preview).muted())
					.child(
						button(format!("{DELETE_PREFIX}{name}"), "Delete")
							.icon("trash")
							.variant("danger"),
					),
			);
		}
		list = list.child(input(ADD_ACTION, "name text, Enter to save"));
		if let Err(error) = ui::register_slot(POPOVER_SLOT, &[list.into()]) {
			log::warn!("could not update the {POPOVER_SLOT} slot: {error:?}");
		}
	}
}

fn split_first_word(input: &str) -> (&str, &str) {
	let input = input.trim();
	input.split_once(char::is_whitespace).unwrap_or((input, ""))
}

fn shortcut_key(name: &str) -> String {
	format!("{SHORTCUT_PREFIX}{name}")
}

fn normalize_name(raw: &str) -> String {
	raw.trim().trim_start_matches(USER_DELIMITER).to_ascii_lowercase()
}

fn load_rules() -> Rules {
	let mut rules = Rules::default();
	for group in [FACES, FLIP, USER] {
		if let Ok(Some(value)) = storage::get(&format!("{SETTING_PREFIX}{group}")) {
			rules.set(group, value == b"1");
		}
	}
	let mut names: Vec<String> = storage::keys()
		.unwrap_or_default()
		.into_iter()
		.filter_map(|key| key.strip_prefix(SHORTCUT_PREFIX).map(str::to_owned))
		.collect();
	names.sort();
	rules.shortcuts = names
		.into_iter()
		.filter_map(|name| match storage::get(&shortcut_key(&name)) {
			Ok(Some(bytes)) => Some((name, String::from_utf8_lossy(&bytes).into_owned())),
			_ => None,
		})
		.collect();
	rules
}

fn is_name_char(character: char) -> bool {
	character.is_ascii_lowercase() || character.is_ascii_digit() || matches!(character, '_' | '-')
}

fn expand(content: &str, rules: &Rules) -> String {
	let mut output = String::with_capacity(content.len());
	let mut index = 0;
	while index < content.len() {
		let rest = &content[index..];
		if let Some(length) = skip_code(rest).or_else(|| skip_link(content, index)) {
			output.push_str(&rest[..length]);
			index += length;
			continue;
		}
		if starts_at_boundary(content, index)
			&& let Some((length, expansion)) = match_shortcut(rest, rules)
		{
			output.push_str(expansion);
			index += length;
			continue;
		}
		let character = rest.chars().next().unwrap_or_default();
		output.push(character);
		index += character.len_utf8();
	}
	output
}

fn backtick_run(text: &str) -> usize {
	text.bytes().take_while(|&byte| byte == b'`').count()
}

fn skip_code(rest: &str) -> Option<usize> {
	if !rest.starts_with('`') {
		return None;
	}
	let run = backtick_run(rest);
	let mut search = run;
	while let Some(found) = rest[search..].find('`') {
		let start = search + found;
		let closing = backtick_run(&rest[start..]);
		if closing == run {
			return Some(start + closing);
		}
		search = start + closing;
	}
	Some(if run >= 3 { rest.len() } else { run })
}

fn skip_link(content: &str, index: usize) -> Option<usize> {
	if !starts_at_boundary(content, index) {
		return None;
	}
	let rest = &content[index..];
	let token = &rest[..rest.find(char::is_whitespace).unwrap_or(rest.len())];
	token.contains("://").then_some(token.len())
}

fn starts_at_boundary(content: &str, index: usize) -> bool {
	content[..index]
		.chars()
		.next_back()
		.is_none_or(|previous| previous.is_whitespace() || previous == '(')
}

fn ends_at_boundary(after: &str) -> bool {
	after
		.chars()
		.next()
		.is_none_or(|next| next.is_whitespace() || TRAILING_PUNCTUATION.contains(next))
}

fn match_shortcut<'a>(rest: &'a str, rules: &'a Rules) -> Option<(usize, &'a str)> {
	let escaped = rest.starts_with('\\');
	let body = &rest[usize::from(escaped)..];
	let delimiter = body
		.chars()
		.next()
		.filter(|&character| character == ':' || character == USER_DELIMITER)?;
	let after_delimiter = &body[1..];
	let name_length = after_delimiter
		.find(|character| !is_name_char(character))
		.unwrap_or(after_delimiter.len());
	let name = &after_delimiter[..name_length];
	if name.is_empty() || name.len() > MAX_NAME_CHARS {
		return None;
	}
	let mut consumed = 1 + name_length;
	if delimiter == ':' {
		if !after_delimiter[name_length..].starts_with(':') {
			return None;
		}
		consumed += 1;
	}
	if !ends_at_boundary(&body[consumed..]) {
		return None;
	}
	let replacement = rules.lookup(delimiter, name)?;
	let length = usize::from(escaped) + consumed;
	Some((length, if escaped { &body[..consumed] } else { replacement }))
}

export_plugin!(TextShortcuts);
