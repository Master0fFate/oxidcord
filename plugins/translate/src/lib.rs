use std::collections::{HashMap, VecDeque};

use plugin_sdk::model::{
	ButtonVariant, MessageFull, Space, TextSize, Tone, UiContext, ViewEvent, ViewEventKind,
};
use plugin_sdk::patch::Patch;
use plugin_sdk::types::{FetchRequest, SettingChange};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, HostError, Md, MessageRef, Plugin, PluginExt, Reply, actions, client, data, export_plugin, host,
	log, menu, mount, storage,
};
use serde_json::{Value, json};

const GOOGLE_URL: &str = "https://translate.googleapis.com/translate_a/single";
const DEEPL_URL: &str = "https://api-free.deepl.com/v2/translate";
const SETTING_KEYS: [&str; 6] = [
	"service",
	"deeplApiKey",
	"receivedInput",
	"receivedOutput",
	"sentInput",
	"sentOutput",
];
const ACCESSORY_POINT: &str = "message-accessory";
const TRANSLATE_BUTTON: &str = "translate";
const DRAFT_BUTTON: &str = "draft";
const DISMISS_BUTTON: &str = "dismiss";
const MAX_SOURCE_CHARS: usize = 4000;
const KEPT_TRANSLATIONS: usize = 100;
const TOAST_MS: u32 = 3000;

enum Service {
	Google,
	Deepl,
}

struct Config {
	service: Service,
	deepl_key: String,
	received_input: String,
	received_output: String,
	sent_input: String,
	sent_output: String,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			service: Service::Google,
			deepl_key: String::new(),
			received_input: "auto".to_owned(),
			received_output: "en".to_owned(),
			sent_input: "auto".to_owned(),
			sent_output: "en".to_owned(),
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		match key {
			"service" => {
				self.service = if value == "deepl" {
					Service::Deepl
				} else {
					Service::Google
				};
			}
			"deeplApiKey" => self.deepl_key = value.to_owned(),
			"receivedInput" => self.received_input = language(value, "auto"),
			"receivedOutput" => self.received_output = language(value, "en"),
			"sentInput" => self.sent_input = language(value, "auto"),
			"sentOutput" => self.sent_output = language(value, "en"),
			_ => {}
		}
	}
}

fn language(value: &str, fallback: &str) -> String {
	if value.is_empty() { fallback } else { value }.to_owned()
}

struct Translated {
	text: String,
	source: String,
}

struct Shown {
	message: MessageRef,
	translated: Translated,
}

#[derive(Default)]
struct Translate {
	config: Config,
	shown: HashMap<u64, Shown>,
	order: VecDeque<u64>,
}

fn describe(error: &HostError) -> String {
	match error {
		HostError::Denied(_) => "A permission is missing".to_owned(),
		HostError::RateLimited => "Too many requests, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "The translation service took too long".to_owned(),
		HostError::Unavailable => "Not available right now".to_owned(),
	}
}

fn encode(text: &str) -> String {
	let mut out = String::with_capacity(text.len() * 3);
	for byte in text.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
			out.push(char::from(byte));
		} else {
			out.push_str(&format!("%{byte:02X}"));
		}
	}
	out
}

fn send(request: &FetchRequest) -> Result<Vec<u8>, String> {
	let response = host::fetch(request).map_err(|error| describe(&error))?;
	match response.status {
		200 => Ok(response.body),
		403 => Err("The translation service refused the request, check your API key".to_owned()),
		456 => Err("The DeepL quota is used up".to_owned()),
		status => Err(format!("The translation service answered {status}")),
	}
}

fn google(text: &str, from: &str, to: &str) -> Result<Translated, String> {
	let request = FetchRequest {
		method: "POST".to_owned(),
		url: format!(
			"{GOOGLE_URL}?client=gtx&dt=t&sl={}&tl={}",
			encode(from),
			encode(to)
		),
		headers: vec![(
			"content-type".to_owned(),
			"application/x-www-form-urlencoded".to_owned(),
		)],
		body: Some(format!("q={}", encode(text)).into_bytes()),
	};
	let answer: Value =
		serde_json::from_slice(&send(&request)?).map_err(|_| "Unreadable answer".to_owned())?;
	let translated: String = answer
		.get(0)
		.and_then(Value::as_array)
		.map(|segments| {
			segments
				.iter()
				.filter_map(|segment| segment.get(0)?.as_str())
				.collect()
		})
		.unwrap_or_default();
	if translated.is_empty() {
		return Err("The translation came back empty".to_owned());
	}
	let source = answer.get(2).and_then(Value::as_str).unwrap_or(from);
	Ok(Translated {
		text: translated,
		source: source.to_owned(),
	})
}

fn deepl_target(code: &str) -> String {
	match code.to_ascii_lowercase().as_str() {
		"en" => "EN-US".to_owned(),
		"pt" => "PT-BR".to_owned(),
		other => other.to_ascii_uppercase(),
	}
}

fn deepl(key: &str, text: &str, from: &str, to: &str) -> Result<Translated, String> {
	if key.is_empty() {
		return Err("Set a DeepL API key in the plugin settings".to_owned());
	}
	let mut payload = json!({ "text": [text], "target_lang": deepl_target(to) });
	if !from.eq_ignore_ascii_case("auto") {
		payload["source_lang"] = json!(from.split('-').next().unwrap_or(from).to_ascii_uppercase());
	}
	let request = FetchRequest {
		method: "POST".to_owned(),
		url: DEEPL_URL.to_owned(),
		headers: vec![
			("authorization".to_owned(), format!("DeepL-Auth-Key {key}")),
			("content-type".to_owned(), "application/json".to_owned()),
		],
		body: Some(payload.to_string().into_bytes()),
	};
	let answer: Value =
		serde_json::from_slice(&send(&request)?).map_err(|_| "Unreadable answer".to_owned())?;
	let first = answer
		.get("translations")
		.and_then(|translations| translations.get(0))
		.ok_or("The translation came back empty")?;
	Ok(Translated {
		text: first
			.get("text")
			.and_then(Value::as_str)
			.ok_or("The translation came back empty")?
			.to_owned(),
		source: first
			.get("detected_source_language")
			.and_then(Value::as_str)
			.unwrap_or(from)
			.to_lowercase(),
	})
}

fn message_ref(context: &UiContext) -> Option<MessageRef> {
	Some(MessageRef {
		id: context.message_id?,
		channel_id: context.channel_id?,
		guild_id: context.guild_id,
	})
}

impl Translate {
	fn translate(&self, text: &str, from: &str, to: &str) -> Result<Translated, String> {
		let text: String = text.chars().take(MAX_SOURCE_CHARS).collect();
		match self.config.service {
			Service::Google => google(&text, from, to),
			Service::Deepl => deepl(&self.config.deepl_key, &text, from, to),
		}
	}

	fn translate_message(&mut self, context: &UiContext) -> Result<(), String> {
		let message = message_ref(context).ok_or("Nothing to translate")?;
		let full = client::get_message(message.channel_id, message.id)
			.map_err(|error| describe(&error))?
			.ok_or("That message is not available")?;
		let text = full.view.content.trim();
		if text.is_empty() {
			return Err("The message has no text".to_owned());
		}
		let translated = self.translate(text, &self.config.received_input, &self.config.received_output)?;
		self.remember(message, translated);
		actions::invalidate_message(message).map_err(|error| describe(&error))
	}

	fn translate_draft(&self) -> Result<(), String> {
		let draft = data::composer_get_draft().map_err(|error| describe(&error))?;
		if draft.trim().is_empty() {
			return Err("The composer is empty".to_owned());
		}
		let translated = self.translate(&draft, &self.config.sent_input, &self.config.sent_output)?;
		actions::composer_set_draft(&translated.text).map_err(|error| describe(&error))
	}

	fn remember(&mut self, message: MessageRef, translated: Translated) {
		if self
			.shown
			.insert(message.id, Shown { message, translated })
			.is_none()
		{
			self.order.push_back(message.id);
			if self.order.len() > KEPT_TRANSLATIONS
				&& let Some(oldest) = self.order.pop_front()
			{
				self.shown.remove(&oldest);
			}
		}
	}

	fn dismiss(&mut self, message_id: u64) {
		let Some(shown) = self.shown.remove(&message_id) else {
			return;
		};
		self.order.retain(|id| *id != message_id);
		if let Err(error) = actions::invalidate_message(shown.message) {
			log::warn!("could not refresh message {message_id}: {error:?}");
		}
	}
}

fn accessory(translated: &Translated) -> Node {
	view::column()
		.gap(Space::Xs)
		.child(
			view::row()
				.gap(Space::Xs)
				.child(view::icon("languages").tone(Tone::Muted))
				.child(
					view::text(format!("Translated from {}", translated.source))
						.size(TextSize::Xs)
						.muted(),
				),
		)
		.child(view::text(translated.text.clone()))
		.child(view::button(DISMISS_BUTTON, "Dismiss").variant(ButtonVariant::Link))
}

impl Plugin for Translate {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		let mounted = mount::mount(
			mount::MESSAGE_ACTION,
			TRANSLATE_BUTTON,
			[view::icon_button(TRANSLATE_BUTTON, "languages", "Translate")],
			mount::Options::new("Translate"),
		)
		.and_then(|()| {
			mount::mount(
				mount::COMPOSER_TOOLBAR,
				DRAFT_BUTTON,
				[view::icon_button(DRAFT_BUTTON, "languages", "Translate draft")],
				mount::Options::new("Translate draft"),
			)
		})
		.and_then(|()| {
			menu::set(
				menu::MESSAGE,
				[menu::item(TRANSLATE_BUTTON, "Translate")
					.icon("languages")
					.when(menu::When::HAS_CONTENT)],
			)
		});
		if let Err(error) = mounted {
			log::warn!("could not add the translate controls: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"service":{"type":"string","title":"Service","description":"Google Translate needs no key. DeepL Free needs an API key","enum":["google","deepl"],"default":"google"},"deeplApiKey":{"type":"string","title":"DeepL API key","description":"From deepl.com/your-account"},"receivedInput":{"type":"string","title":"Incoming messages from","description":"Language code, or auto","default":"auto"},"receivedOutput":{"type":"string","title":"Incoming messages to","description":"Language code","default":"en"},"sentInput":{"type":"string","title":"Your messages from","description":"Language code, or auto","default":"auto"},"sentOutput":{"type":"string","title":"Your messages to","description":"Language code","default":"en"}}}"#
			.to_owned()
	}
}

impl PluginExt for Translate {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click {
			return Reply::none();
		}
		let outcome = match (event.point.as_str(), event.node.as_str()) {
			(mount::MESSAGE_ACTION, TRANSLATE_BUTTON) => self.translate_message(&event.context),
			(mount::COMPOSER_TOOLBAR, DRAFT_BUTTON) => self.translate_draft(),
			(ACCESSORY_POINT, DISMISS_BUTTON) => {
				if let Ok(message_id) = event.mount.parse() {
					self.dismiss(message_id);
				}
				Ok(())
			}
			_ => Ok(()),
		};
		match outcome {
			Ok(()) => Reply::none(),
			Err(message) => Reply::none().toast(message),
		}
	}

	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		if menu != menu::MESSAGE || key != TRANSLATE_BUTTON {
			return;
		}
		if let Err(message) = self.translate_message(context)
			&& let Err(error) = mount::toast([view::text(message)], TOAST_MS)
		{
			log::warn!("could not show the toast: {error:?}");
		}
	}

	fn intercept_message(&mut self, msg: &MessageFull, _content: Vec<Md>) -> Patch {
		match self.shown.get(&msg.view.id) {
			Some(shown) => Patch::keep().accessory([accessory(&shown.translated)]),
			None => Patch::keep(),
		}
	}
}

export_plugin!(Translate, ext);
