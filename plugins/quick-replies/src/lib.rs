use plugin_sdk::ui::{self, button, column, input, row, text};
use plugin_sdk::{OutgoingMessage, Plugin, Ui, export_plugin, host, log, storage};

const SNIPPET_PREFIX: &str = "snippet.";
const TOOLBAR_SLOT: &str = "composer-toolbar";
const POPOVER_SLOT: &str = "popover";
const OPEN_ACTION: &str = "open-snippets";
const ADD_ACTION: &str = "add-snippet";
const USE_PREFIX: &str = "use:";
const DELETE_PREFIX: &str = "delete:";
const MAX_NAME_CHARS: usize = 32;
const PREVIEW_CHARS: usize = 40;

#[derive(Default)]
struct QuickReplies;

impl Plugin for QuickReplies {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = ui::register_command("snip", "Send a saved snippet: /snip <name>") {
			log::warn!("could not register /snip: {error:?}");
		}
		let toolbar = [Ui::from(button(OPEN_ACTION, "Snippets").icon("message-square"))];
		if let Err(error) = ui::register_slot(TOOLBAR_SLOT, &toolbar) {
			log::warn!("could not register the {TOOLBAR_SLOT} slot: {error:?}");
		}
		Ok(())
	}

	fn on_ui_action(&mut self, id: &str, payload: &str) -> Vec<Ui> {
		if id == OPEN_ACTION {
			show_popover();
		} else if id == ADD_ACTION {
			add_snippet(payload);
			show_popover();
		} else if let Some(name) = id.strip_prefix(USE_PREFIX) {
			if let Err(error) = send_from_toolbar(name) {
				log::warn!("snippet {name} not sent: {error}");
			}
			close_popover();
		} else if let Some(name) = id.strip_prefix(DELETE_PREFIX) {
			if let Err(error) = storage::delete(&snippet_key(name)) {
				log::warn!("could not delete snippet {name}: {error:?}");
			}
			show_popover();
		}
		Vec::new()
	}

	fn on_command(&mut self, name: &str, args: &str, channel_id: u64) -> Result<(), String> {
		if name != "snip" {
			return Ok(());
		}
		send_snippet(args.trim(), channel_id)
	}
}

fn snippet_key(name: &str) -> String {
	format!("{SNIPPET_PREFIX}{name}")
}

fn load_snippet(name: &str) -> Result<String, String> {
	match storage::get(&snippet_key(name)) {
		Ok(Some(bytes)) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
		Ok(None) => Err(format!("no snippet named {name}")),
		Err(error) => Err(format!("storage unavailable: {error:?}")),
	}
}

fn send_snippet(name: &str, channel_id: u64) -> Result<(), String> {
	let content = load_snippet(name)?;
	host::send_message(&OutgoingMessage {
		channel_id,
		content,
		reply_to: None,
	})
	.map(drop)
	.map_err(|error| format!("could not send: {error:?}"))
}

fn send_from_toolbar(name: &str) -> Result<(), String> {
	match host::current_channel() {
		Ok(Some((_, channel_id))) => send_snippet(name, channel_id),
		_ => Err("no channel is open".to_owned()),
	}
}

fn add_snippet(payload: &str) {
	let Some((name, content)) = payload.trim().split_once(char::is_whitespace) else {
		return;
	};
	let name: String = name
		.to_ascii_lowercase()
		.chars()
		.filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
		.take(MAX_NAME_CHARS)
		.collect();
	if name.is_empty() {
		return;
	}
	if let Err(error) = storage::set(&snippet_key(&name), content.trim().as_bytes()) {
		log::warn!("could not save snippet {name}: {error:?}");
	}
}

fn snippets() -> Vec<(String, String)> {
	let mut names: Vec<String> = storage::keys()
		.unwrap_or_default()
		.into_iter()
		.filter_map(|key| key.strip_prefix(SNIPPET_PREFIX).map(str::to_owned))
		.collect();
	names.sort();
	names
		.into_iter()
		.filter_map(|name| load_snippet(&name).ok().map(|content| (name, content)))
		.collect()
}

fn show_popover() {
	let mut list = column().gap(4).padding(8);
	for (name, content) in snippets() {
		let preview: String = content.chars().take(PREVIEW_CHARS).collect();
		list = list.child(
			row()
				.gap(8)
				.child(button(format!("{USE_PREFIX}{name}"), name.as_str()))
				.child(text(preview).muted())
				.child(
					button(format!("{DELETE_PREFIX}{name}"), "Delete")
						.icon("trash")
						.variant("danger"),
				),
		);
	}
	list = list.child(input(ADD_ACTION, "name text, Enter to save"));
	register_popover(&[list.into()]);
}

fn close_popover() {
	register_popover(&[]);
}

fn register_popover(tree: &[Ui]) {
	if let Err(error) = ui::register_slot(POPOVER_SLOT, tree) {
		log::warn!("could not update the {POPOVER_SLOT} slot: {error:?}");
	}
}

export_plugin!(QuickReplies);
