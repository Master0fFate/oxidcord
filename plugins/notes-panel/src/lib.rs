use plugin_sdk::md::Color;
use plugin_sdk::model::{ButtonVariant, Radius, Space, TextSize, Tone, UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Plugin, PluginExt, Reply, client, export_plugin, log, menu, mount, now_ms, storage, timer};

const PAGE: &str = "notes";
const CHIP: &str = "notes-chip";
const SAVE_ITEM: &str = "save";
const ADD_FORM: &str = "add-form";
const TITLE_FIELD: &str = "title";
const BODY_FIELD: &str = "body";
const DELETE_PREFIX: &str = "delete:";
const CLEAR_ALL: &str = "clear-all";
const NOTE_PREFIX: &str = "note.";
const NEXT_KEY: &str = "next-id";
const CLOCK_TAG: &str = "clock";
const CLOCK_EVERY_MS: u32 = 30_000;
const MAX_NOTES: usize = 200;
const MAX_TITLE_CHARS: usize = 80;
const MAX_BODY_CHARS: usize = 2_000;
const PREVIEW_CHARS: usize = 60;

struct Note {
	id: u64,
	title: String,
	body: String,
	saved_ms: i64,
}

#[derive(Default)]
struct NotesPanel {
	notes: Vec<Note>,
	next_id: u64,
}

fn clip(text: &str, max_chars: usize) -> &str {
	text.char_indices()
		.nth(max_chars)
		.map_or(text, |(end, _)| &text[..end])
}

fn note_key(id: u64) -> String {
	format!("{NOTE_PREFIX}{id}")
}

fn encode(note: &Note) -> Vec<u8> {
	format!("{}\n{}\n{}", note.saved_ms, note.title, note.body).into_bytes()
}

fn decode(id: u64, bytes: &[u8]) -> Option<Note> {
	let text = String::from_utf8_lossy(bytes);
	let mut parts = text.splitn(3, '\n');
	let saved_ms = parts.next()?.parse().ok()?;
	let title = parts.next()?.to_owned();
	let body = parts.next().unwrap_or_default().to_owned();
	Some(Note {
		id,
		title,
		body,
		saved_ms,
	})
}

fn ago(saved_ms: i64) -> String {
	let minutes = (now_ms() - saved_ms).max(0) / 60_000;
	match minutes {
		0 => "just now".to_owned(),
		1..=59 => format!("{minutes} min ago"),
		_ => format!("{} h ago", minutes / 60),
	}
}

impl NotesPanel {
	fn load(&mut self) {
		self.next_id = storage::get(NEXT_KEY)
			.ok()
			.flatten()
			.and_then(|bytes| String::from_utf8(bytes).ok()?.parse().ok())
			.unwrap_or(1);
		let mut ids: Vec<u64> = storage::keys()
			.unwrap_or_default()
			.iter()
			.filter_map(|key| key.strip_prefix(NOTE_PREFIX)?.parse().ok())
			.collect();
		ids.sort_unstable();
		self.notes = ids
			.into_iter()
			.filter_map(|id| decode(id, &storage::get(&note_key(id)).ok()??))
			.collect();
	}

	fn add(&mut self, title: &str, body: &str) -> bool {
		let title = clip(title.trim(), MAX_TITLE_CHARS);
		let body = clip(body.trim(), MAX_BODY_CHARS);
		if title.is_empty() && body.is_empty() {
			return false;
		}
		let note = Note {
			id: self.next_id,
			title: if title.is_empty() {
				clip(body, PREVIEW_CHARS).to_owned()
			} else {
				title.to_owned()
			},
			body: body.to_owned(),
			saved_ms: now_ms(),
		};
		self.next_id += 1;
		if let Err(error) = storage::set(&note_key(note.id), &encode(&note))
			.and_then(|()| storage::set(NEXT_KEY, self.next_id.to_string().as_bytes()))
		{
			log::warn!("could not save note: {error:?}");
			return false;
		}
		self.notes.push(note);
		if self.notes.len() > MAX_NOTES {
			let oldest = self.notes.remove(0);
			storage::delete(&note_key(oldest.id)).ok();
		}
		true
	}

	fn delete(&mut self, id: u64) {
		self.notes.retain(|note| note.id != id);
		if let Err(error) = storage::delete(&note_key(id)) {
			log::warn!("could not delete note {id}: {error:?}");
		}
	}

	fn clear(&mut self) {
		for note in std::mem::take(&mut self.notes) {
			storage::delete(&note_key(note.id)).ok();
		}
	}

	fn list(&self) -> Node {
		if self.notes.is_empty() {
			return view::text("No notes yet. Add one or save a message from its menu.").muted();
		}
		let rows = self.notes.iter().rev().map(|note| {
			view::row()
				.gap(Space::Md)
				.padding(Space::Md)
				.radius(Radius::Md)
				.background(Color::token("background-secondary"))
				.fill()
				.child(view::icon("sticky-note").tone(Tone::Brand))
				.child(
					view::column()
						.gap(Space::Xs)
						.fill()
						.child(view::text(note.title.clone()).bold())
						.child(view::text(clip(&note.body, PREVIEW_CHARS)).muted())
						.child(view::text(ago(note.saved_ms)).size(TextSize::Tiny).muted()),
				)
				.child(
					view::icon_button(format!("{DELETE_PREFIX}{}", note.id), "trash", "Delete")
						.tone(Tone::Danger),
				)
		});
		view::scroll().gap(Space::Sm).fill().children(rows)
	}

	fn add_form(&self) -> Node {
		view::form()
			.key(ADD_FORM)
			.gap(Space::Md)
			.child(view::input(TITLE_FIELD, "Title"))
			.child(view::textarea(BODY_FIELD, "Write a note"))
			.child(
				view::row().gap(Space::Sm).child(
					view::button("add", "Add note")
						.icon("plus")
						.tone(Tone::Brand)
						.submit(),
				),
			)
	}

	fn page(&self) -> Vec<Node> {
		let header = view::row()
			.gap(Space::Md)
			.fill()
			.child(view::icon("notebook").tone(Tone::Brand))
			.child(view::heading("Notes"))
			.child(view::badge(self.notes.len().to_string()))
			.child(view::spacer())
			.child(
				view::button(CLEAR_ALL, "Clear all")
					.variant(ButtonVariant::Ghost)
					.tone(Tone::Danger)
					.disabled(self.notes.is_empty()),
			);
		let tabs = view::tabs()
			.fill()
			.tab("Notes", self.list())
			.tab("Add", self.add_form())
			.tab(
				"About",
				view::markdown(
					"Notes stay on this device. Right click a message and pick **Save to notes** to keep it here.",
				),
			);
		vec![
			view::column()
				.gap(Space::Lg)
				.padding(Space::Xl)
				.fill()
				.child(header)
				.child(tabs),
		]
	}

	fn chip(&self) -> Vec<Node> {
		let last = self.notes.last().map_or_else(
			|| "no notes".to_owned(),
			|note| format!("last {}", ago(note.saved_ms)),
		);
		vec![
			view::row()
				.gap(Space::Xs)
				.child(view::icon("notebook").tone(Tone::Muted))
				.child(
					view::text(format!("{} notes, {last}", self.notes.len()))
						.size(TextSize::Xs)
						.muted(),
				),
		]
	}

	fn publish(&self) {
		let page = mount::update(mount::MAIN_VIEW, PAGE, self.page());
		let chip = mount::update(mount::STATUS_BAR, CHIP, self.chip());
		if let Err(error) = page.and(chip) {
			log::warn!("could not refresh the notes views: {error:?}");
		}
	}

	fn mount_all(&self) {
		let mounted = mount::mount(
			mount::APP_NAV,
			PAGE,
			Vec::<Node>::new(),
			mount::Options::new("Notes").icon("notebook"),
		)
		.and_then(|()| mount::mount(mount::MAIN_VIEW, PAGE, self.page(), mount::Options::new("Notes")))
		.and_then(|()| mount::mount(mount::STATUS_BAR, CHIP, self.chip(), mount::Options::new("Notes")))
		.and_then(|()| {
			menu::set(
				menu::MESSAGE,
				[menu::item(SAVE_ITEM, "Save to notes")
					.icon("sticky-note")
					.when(menu::When::HAS_CONTENT)],
			)
		});
		if let Err(error) = mounted {
			log::warn!("could not mount the notes views: {error:?}");
		}
	}

	fn save_message(&mut self, context: &UiContext) -> &'static str {
		let (Some(channel_id), Some(message_id)) = (context.channel_id, context.message_id) else {
			return "Nothing to save";
		};
		let message = match client::get_message(channel_id, message_id) {
			Ok(Some(message)) => message,
			Ok(None) => return "That message is not available",
			Err(error) => {
				log::warn!("could not read message {message_id}: {error:?}");
				return "Could not read that message";
			}
		};
		let title = format!("From {}", message.view.author.display_name);
		if !self.add(&title, &message.view.content) {
			return "That message has no text";
		}
		self.publish();
		"Saved to notes"
	}
}

impl Plugin for NotesPanel {
	fn init(&mut self) -> Result<(), String> {
		self.load();
		self.mount_all();
		if let Err(error) = timer::every(CLOCK_EVERY_MS, CLOCK_TAG) {
			log::warn!("could not start the clock: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for NotesPanel {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		match (&event.kind, event.node.as_str()) {
			(ViewEventKind::Submit(values), ADD_FORM) => {
				let field = |name: &str| {
					values
						.iter()
						.find(|(key, _)| key == name)
						.map_or("", |(_, value)| value.as_str())
				};
				if !self.add(field(TITLE_FIELD), field(BODY_FIELD)) {
					return Reply::none().toast("Write a title or some text first");
				}
				self.publish();
				Reply::none().toast("Note added")
			}
			(ViewEventKind::Click, CLEAR_ALL) => {
				self.clear();
				self.publish();
				Reply::none().toast("Notes cleared")
			}
			(ViewEventKind::Click, node) => {
				if let Some(id) = node.strip_prefix(DELETE_PREFIX).and_then(|id| id.parse().ok()) {
					self.delete(id);
					self.publish();
				}
				Reply::none()
			}
			_ => Reply::none(),
		}
	}

	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		if menu != menu::MESSAGE || key != SAVE_ITEM {
			return;
		}
		let outcome = self.save_message(context);
		if let Err(error) = mount::toast([view::text(outcome)], 0) {
			log::warn!("could not show the toast: {error:?}");
		}
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == CLOCK_TAG {
			self.publish();
		}
	}
}

export_plugin!(NotesPanel, ext);
