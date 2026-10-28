use std::collections::BTreeMap;

use plugin_sdk::model::{Space, Tone, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{OutgoingMessage, Plugin, PluginExt, Reply, export_plugin, host, log, mount, storage, ui};

const COMMAND: &str = "tags";
const TAG_PREFIX: &str = "tag.";
const DELETE_PREFIX: &str = "del:";
const TAG_DESCRIPTION: &str = "Send a saved tag";
const DELETED_DESCRIPTION: &str = "Deleted tag, gone after a restart";
const MAX_TAGS: usize = 100;
const MAX_NAME_CHARS: usize = 32;
const MAX_TEXT_CHARS: usize = 2000;
const PREVIEW_CHARS: usize = 60;
const TOAST_MS: u32 = 3000;

#[derive(Default)]
struct CustomCommands {
	tags: BTreeMap<String, String>,
}

struct Placeholder {
	start: usize,
	end: usize,
	name: String,
	default: Option<String>,
}

fn split_first_word(text: &str) -> (&str, &str) {
	text.split_once(char::is_whitespace)
		.map_or((text, ""), |(word, rest)| (word, rest.trim_start()))
}

fn normalize_name(raw: &str) -> Option<String> {
	let name = raw.trim().to_ascii_lowercase();
	let valid = !name.is_empty()
		&& name.chars().count() <= MAX_NAME_CHARS
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_'));
	(valid && name != COMMAND).then_some(name)
}

fn placeholders(template: &str) -> Vec<Placeholder> {
	let mut found = Vec::new();
	let mut from = 0;
	while let Some(open) = template[from..].find("{{") {
		let start = from + open;
		let Some(close) = template[start + 2..].find("}}") else {
			break;
		};
		let end = start + 2 + close + 2;
		let (name, default) = match template[start + 2..end - 2].split_once('=') {
			Some((name, default)) => (name, Some(default.trim().to_owned())),
			None => (&template[start + 2..end - 2], None),
		};
		let name = name.trim().to_lowercase();
		if !name.is_empty() {
			found.push(Placeholder {
				start,
				end,
				name,
				default,
			});
		}
		from = end;
	}
	found
}

fn render(template: &str, args: &str) -> Result<String, String> {
	let found = placeholders(template);
	let mut names: Vec<&str> = Vec::new();
	for placeholder in &found {
		if !names.contains(&placeholder.name.as_str()) {
			names.push(&placeholder.name);
		}
	}
	let mut values: BTreeMap<&str, &str> = BTreeMap::new();
	let mut rest = args.trim();
	for (index, name) in names.iter().enumerate() {
		if index + 1 == names.len() {
			values.insert(name, rest);
			rest = "";
		} else {
			let (word, tail) = split_first_word(rest);
			values.insert(name, word);
			rest = tail;
		}
	}
	let mut out = String::with_capacity(template.len());
	let mut cursor = 0;
	for placeholder in &found {
		out.push_str(&template[cursor..placeholder.start]);
		let given = values
			.get(placeholder.name.as_str())
			.copied()
			.filter(|value| !value.is_empty());
		match given.or(placeholder.default.as_deref()) {
			Some(value) => out.push_str(value),
			None => return Err(format!("{} needs a value", placeholder.name)),
		}
		cursor = placeholder.end;
	}
	out.push_str(&template[cursor..]);
	Ok(out.replace("\\n", "\n").chars().take(MAX_TEXT_CHARS).collect())
}

fn toast(text: &str) {
	if let Err(error) = mount::toast([view::text(text)], TOAST_MS) {
		log::warn!("could not show the toast: {error:?}");
	}
}

impl CustomCommands {
	fn save(&mut self, name: String, text: &str) -> Result<(), String> {
		let text: String = text.trim().chars().take(MAX_TEXT_CHARS).collect();
		if text.is_empty() {
			return Err("a tag needs some text".to_owned());
		}
		if !self.tags.contains_key(&name) && self.tags.len() >= MAX_TAGS {
			return Err(format!("you can keep at most {MAX_TAGS} tags"));
		}
		storage::set(&format!("{TAG_PREFIX}{name}"), text.as_bytes())
			.map_err(|error| format!("{error:?}"))?;
		ui::register_command(&name, TAG_DESCRIPTION).map_err(|error| format!("{error:?}"))?;
		self.tags.insert(name, text);
		Ok(())
	}

	fn delete(&mut self, name: &str) -> bool {
		if self.tags.remove(name).is_none() {
			return false;
		}
		if let Err(error) = storage::delete(&format!("{TAG_PREFIX}{name}")) {
			log::warn!("could not delete tag {name}: {error:?}");
		}
		if let Err(error) = ui::register_command(name, DELETED_DESCRIPTION) {
			log::warn!("could not retire /{name}: {error:?}");
		}
		true
	}

	fn manage(&mut self, args: &str) -> Result<(), String> {
		let (action, rest) = split_first_word(args.trim());
		match action {
			"create" => {
				let (name, text) = split_first_word(rest);
				let name = normalize_name(name)
					.ok_or("use /tags create <name> <text> with a name of letters, digits, - or _")?;
				self.save(name.clone(), text)?;
				toast(&format!("Saved /{name}"));
				Ok(())
			}
			"delete" => {
				let name = rest.trim().to_ascii_lowercase();
				if self.delete(&name) {
					toast(&format!("Deleted /{name}"));
					Ok(())
				} else {
					Err(format!("no tag named {name}"))
				}
			}
			"list" => mount::open_modal("Tags", self.list()).map_err(|error| format!("{error:?}")),
			_ => Err("use /tags create, list or delete".to_owned()),
		}
	}

	fn list(&self) -> Vec<Node> {
		if self.tags.is_empty() {
			return vec![view::text("No tags yet. Use /tags create <name> <text>").muted()];
		}
		let rows = self.tags.iter().map(|(name, text)| {
			let preview: String = text.replace('\n', " ").chars().take(PREVIEW_CHARS).collect();
			view::row()
				.gap(Space::Md)
				.fill()
				.child(view::text(format!("/{name}")).bold())
				.child(view::text(preview).muted())
				.child(view::spacer())
				.child(
					view::icon_button(format!("{DELETE_PREFIX}{name}"), "trash", "Delete tag")
						.tone(Tone::Danger),
				)
		});
		vec![
			view::column()
				.gap(Space::Sm)
				.padding(Space::Md)
				.fill()
				.children(rows),
		]
	}
}

impl Plugin for CustomCommands {
	fn init(&mut self) -> Result<(), String> {
		for key in storage::keys().unwrap_or_default() {
			let Some(name) = key.strip_prefix(TAG_PREFIX) else {
				continue;
			};
			if let Ok(Some(bytes)) = storage::get(&key)
				&& self.tags.len() < MAX_TAGS
			{
				self.tags
					.insert(name.to_owned(), String::from_utf8_lossy(&bytes).into_owned());
			}
		}
		if let Err(error) =
			ui::register_command(COMMAND, "Manage tags: create <name> <text>, list, delete <name>")
		{
			log::warn!("could not register /{COMMAND}: {error:?}");
		}
		for name in self.tags.keys() {
			if let Err(error) = ui::register_command(name, TAG_DESCRIPTION) {
				log::warn!("could not register /{name}: {error:?}");
			}
		}
		Ok(())
	}

	fn on_command(&mut self, name: &str, args: &str, channel_id: u64) -> Result<(), String> {
		if name == COMMAND {
			return self.manage(args);
		}
		let Some(template) = self.tags.get(name) else {
			toast(&format!("/{name} was deleted"));
			return Ok(());
		};
		let content = render(template, args)?;
		host::send_message(&OutgoingMessage {
			channel_id,
			content,
			reply_to: None,
		})
		.map(drop)
		.map_err(|error| format!("could not send: {error:?}"))
	}
}

impl PluginExt for CustomCommands {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click {
			return Reply::none();
		}
		match event.node.strip_prefix(DELETE_PREFIX) {
			Some(name) if self.delete(name) => Reply::none().replace(self.list()),
			_ => Reply::none(),
		}
	}
}

export_plugin!(CustomCommands, ext);
