use std::collections::HashMap;

use plugin_sdk::ui::{self, button, column, input, row, text};
use plugin_sdk::{DisplayOverride, Plugin, Ui, User, export_plugin, log, storage};

const NOTE_PREFIX: &str = "note.";
const MENU_SLOT: &str = "user-context-menu";
const MODAL_SLOT: &str = "modal";
const ADD_ACTION: &str = "add-note";
const INPUT_ACTION: &str = "note-input";
const SAVE_ACTION: &str = "save-note";
const CANCEL_ACTION: &str = "cancel-note";
const BADGE_CHARS: usize = 20;
const MAX_NOTE_CHARS: usize = 500;

#[derive(Default)]
struct UserNotes {
	notes: HashMap<u64, String>,
	editing: Option<u64>,
	draft: String,
}

impl UserNotes {
	fn open_editor(&mut self, user_id: u64) {
		self.editing = Some(user_id);
		self.draft = self.notes.get(&user_id).cloned().unwrap_or_default();
		let editor = column()
			.gap(8)
			.padding(12)
			.child(text("Note").bold())
			.child(input(INPUT_ACTION, "Only you can see this").value(self.draft.as_str()))
			.child(
				row()
					.gap(8)
					.child(button(SAVE_ACTION, "Save").variant("primary"))
					.child(button(CANCEL_ACTION, "Cancel")),
			);
		register_modal(&[editor.into()]);
	}

	fn save(&mut self) {
		let Some(user_id) = self.editing.take() else {
			return;
		};
		let note: String = self.draft.trim().chars().take(MAX_NOTE_CHARS).collect();
		let key = format!("{NOTE_PREFIX}{user_id}");
		let outcome = if note.is_empty() {
			self.notes.remove(&user_id);
			storage::delete(&key)
		} else {
			let saved = storage::set(&key, note.as_bytes());
			self.notes.insert(user_id, note);
			saved
		};
		if let Err(error) = outcome {
			log::warn!("could not save the note: {error:?}");
		}
	}
}

impl Plugin for UserNotes {
	fn init(&mut self) -> Result<(), String> {
		let menu = [Ui::from(button(ADD_ACTION, "Add note").icon("pencil"))];
		if let Err(error) = ui::register_slot(MENU_SLOT, &menu) {
			log::warn!("could not register the {MENU_SLOT} slot: {error:?}");
		}
		for key in storage::keys().unwrap_or_default() {
			let Some(user_id) = key.strip_prefix(NOTE_PREFIX).and_then(|id| id.parse().ok()) else {
				continue;
			};
			if let Ok(Some(bytes)) = storage::get(&key) {
				self.notes
					.insert(user_id, String::from_utf8_lossy(&bytes).into_owned());
			}
		}
		Ok(())
	}

	fn on_ui_action(&mut self, id: &str, payload: &str) -> Vec<Ui> {
		match id {
			ADD_ACTION => {
				if let Ok(user_id) = payload.trim().parse() {
					self.open_editor(user_id);
				}
			}
			INPUT_ACTION => self.draft = payload.to_owned(),
			SAVE_ACTION => {
				self.save();
				register_modal(&[]);
			}
			CANCEL_ACTION => {
				self.editing = None;
				register_modal(&[]);
			}
			_ => {}
		}
		Vec::new()
	}

	fn display_user(&mut self, _guild_id: Option<u64>, user: &User) -> Option<DisplayOverride> {
		let note = self.notes.get(&user.id)?;
		Some(DisplayOverride {
			name: None,
			color: None,
			badge: Some(note.chars().take(BADGE_CHARS).collect()),
		})
	}
}

fn register_modal(tree: &[Ui]) {
	if let Err(error) = ui::register_slot(MODAL_SLOT, tree) {
		log::warn!("could not update the {MODAL_SLOT} slot: {error:?}");
	}
}

export_plugin!(UserNotes);
