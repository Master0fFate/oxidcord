use plugin_sdk::model::{Length, Radius, Space, TextSize, Tone, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Color, OutgoingMessage, Plugin, PluginExt, Reply, SendDecision, export_plugin, log, mount, storage,
};
use regex_lite::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

const STRING_RULES_KEY: &str = "stringRules";
const REGEX_RULES_KEY: &str = "regexRules";
const MOUNT_KEY: &str = "rules";
const SETTINGS_POINT: &str = "settings-page";
const TEST_FIELD: &str = "test";
const ADD_PREFIX: &str = "add:";
const DELETE_PREFIX: &str = "del:";
const MAX_RULES: usize = 200;
const MAX_FIELD_CHARS: usize = 500;
const MAX_CONTENT_BYTES: usize = 4000;
const REGEX_SIZE_LIMIT: usize = 1 << 20;
const LABEL_WIDTH: u32 = 130;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Rule {
	name: String,
	find: String,
	replace: String,
	only_if_includes: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
	Simple,
	Regex,
}

impl Kind {
	fn tag(self) -> &'static str {
		match self {
			Self::Simple => "s",
			Self::Regex => "r",
		}
	}

	fn from_tag(tag: &str) -> Option<Self> {
		match tag {
			"s" => Some(Self::Simple),
			"r" => Some(Self::Regex),
			_ => None,
		}
	}
}

struct Compiled {
	regex: Regex,
	global: bool,
	replacement: String,
}

#[derive(Default)]
struct TextReplace {
	simple: Vec<Rule>,
	regex: Vec<Rule>,
	compiled: Vec<Result<Compiled, String>>,
	sample: String,
}

fn load_rules(key: &str) -> Vec<Rule> {
	let Ok(Some(bytes)) = storage::get(key) else {
		return Vec::new();
	};
	let Ok(items) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) else {
		return Vec::new();
	};
	items
		.into_iter()
		.filter_map(|item| serde_json::from_value(item).ok())
		.take(MAX_RULES)
		.collect()
}

fn split_pattern(find: &str) -> (&str, Option<&str>) {
	let body = find.strip_prefix('/').unwrap_or(find);
	for (at, _) in body.char_indices().skip(1) {
		if let Some(flags) = body[at..].strip_prefix('/')
			&& flags.chars().all(|flag| "gimsuyv".contains(flag))
		{
			return (&body[..at], Some(flags));
		}
	}
	(body, None)
}

fn expand_replacement(replace: &str) -> String {
	let text = replace.replace("\\n", "\n");
	let mut out = String::with_capacity(text.len());
	let mut chars = text.chars().peekable();
	while let Some(character) = chars.next() {
		if character != '$' {
			out.push(character);
			continue;
		}
		match chars.peek().copied() {
			Some('$') => {
				chars.next();
				out.push_str("$$");
			}
			Some('&') => {
				chars.next();
				out.push_str("${0}");
			}
			Some(digit) if digit.is_ascii_digit() => {
				chars.next();
				out.push_str(&format!("${{{digit}}}"));
			}
			Some('<') => {
				chars.next();
				let name: String = chars.by_ref().take_while(|&next| next != '>').collect();
				out.push_str(&format!("${{{name}}}"));
			}
			_ => out.push_str("$$"),
		}
	}
	out
}

fn compile(rule: &Rule) -> Result<Compiled, String> {
	let (pattern, flags) = split_pattern(&rule.find);
	let flags = flags.unwrap_or("g");
	let regex = RegexBuilder::new(pattern)
		.case_insensitive(flags.contains('i'))
		.multi_line(flags.contains('m'))
		.dot_matches_new_line(flags.contains('s'))
		.size_limit(REGEX_SIZE_LIMIT)
		.build()
		.map_err(|error| error.to_string())?;
	Ok(Compiled {
		regex,
		global: flags.contains('g'),
		replacement: expand_replacement(&rule.replace),
	})
}

fn applies(rule: &Rule, text: &str) -> bool {
	!rule.find.is_empty() && (rule.only_if_includes.is_empty() || text.contains(&rule.only_if_includes))
}

fn trim_edges(text: &str) -> &str {
	let text = text.strip_prefix(char::is_whitespace).unwrap_or(text);
	text.strip_suffix(char::is_whitespace).unwrap_or(text)
}

fn clip(text: &str) -> String {
	text.chars().take(MAX_FIELD_CHARS).collect()
}

impl TextReplace {
	fn rules(&self, kind: Kind) -> &Vec<Rule> {
		match kind {
			Kind::Simple => &self.simple,
			Kind::Regex => &self.regex,
		}
	}

	fn rules_mut(&mut self, kind: Kind) -> &mut Vec<Rule> {
		match kind {
			Kind::Simple => &mut self.simple,
			Kind::Regex => &mut self.regex,
		}
	}

	fn recompile(&mut self) {
		self.compiled = self.regex.iter().map(compile).collect();
	}

	fn save(&self, kind: Kind) {
		let key = match kind {
			Kind::Simple => STRING_RULES_KEY,
			Kind::Regex => REGEX_RULES_KEY,
		};
		match serde_json::to_vec(self.rules(kind)) {
			Ok(bytes) => {
				if let Err(error) = storage::set(key, &bytes) {
					log::warn!("could not save the {key}: {error:?}");
				}
			}
			Err(error) => log::warn!("could not encode the {key}: {error}"),
		}
	}

	fn apply(&self, content: &str) -> String {
		let mut text = content.to_owned();
		for rule in &self.simple {
			if !applies(rule, &text) {
				continue;
			}
			let padded = format!(" {text} ");
			let replaced = padded.replace(&rule.find, &rule.replace.replace("\\n", "\n"));
			text = trim_edges(&replaced).to_owned();
		}
		for (rule, compiled) in self.regex.iter().zip(&self.compiled) {
			let Ok(compiled) = compiled else {
				continue;
			};
			if !applies(rule, &text) {
				continue;
			}
			let replacement = compiled.replacement.as_str();
			text = if compiled.global {
				compiled.regex.replace_all(&text, replacement).into_owned()
			} else {
				compiled.regex.replace(&text, replacement).into_owned()
			};
		}
		text.trim().to_owned()
	}

	fn edit(&mut self, kind: Kind, index: usize, field: &str, value: &str) {
		let Some(rule) = self.rules_mut(kind).get_mut(index) else {
			return;
		};
		let value = clip(value);
		match field {
			"name" => rule.name = value,
			"find" => rule.find = value,
			"replace" => rule.replace = value,
			"only" => rule.only_if_includes = value,
			_ => return,
		}
		self.changed(kind);
	}

	fn changed(&mut self, kind: Kind) {
		if kind == Kind::Regex {
			self.recompile();
		}
		self.save(kind);
	}

	fn field(kind: Kind, index: usize, name: &str, label: &str, value: &str) -> Node {
		view::row()
			.gap(Space::Sm)
			.fill()
			.child(view::text(label).muted().width(Length::Px(LABEL_WIDTH)))
			.child(
				view::input(format!("{}:{index}:{name}", kind.tag()), label)
					.value(value)
					.fill(),
			)
	}

	fn card(&self, kind: Kind, index: usize, rule: &Rule) -> Node {
		let find_label = if kind == Kind::Regex { "Pattern" } else { "Find" };
		let mut card = view::column()
			.gap(Space::Xs)
			.padding(Space::Md)
			.radius(Radius::Md)
			.background(Color::token("background-secondary"))
			.fill()
			.child(Self::field(kind, index, "name", "Name", &rule.name))
			.child(Self::field(kind, index, "find", find_label, &rule.find))
			.child(Self::field(kind, index, "replace", "Replace", &rule.replace))
			.child(Self::field(
				kind,
				index,
				"only",
				"Only if includes",
				&rule.only_if_includes,
			));
		if let Some(Err(message)) = (kind == Kind::Regex).then(|| self.compiled.get(index)).flatten() {
			card = card.child(
				view::text(message.clone())
					.size(TextSize::Xs)
					.color(Color::token("danger")),
			);
		}
		card.child(
			view::row().child(view::spacer()).child(
				view::icon_button(
					format!("{DELETE_PREFIX}{}:{index}", kind.tag()),
					"trash",
					"Delete rule",
				)
				.tone(Tone::Danger),
			),
		)
	}

	fn section(&self, kind: Kind, title: &str) -> Node {
		let cards = self
			.rules(kind)
			.iter()
			.enumerate()
			.map(|(index, rule)| self.card(kind, index, rule));
		view::section(title)
			.child(view::column().gap(Space::Md).fill().children(cards))
			.child(
				view::button(format!("{ADD_PREFIX}{}", kind.tag()), "Add rule")
					.icon("plus")
					.disabled(self.rules(kind).len() >= MAX_RULES),
			)
	}

	fn page(&self) -> Vec<Node> {
		let tester = view::column()
			.gap(Space::Sm)
			.child(view::heading("Rule tester"))
			.child(view::input(TEST_FIELD, "Type a message").value(self.sample.as_str()))
			.child(view::text(self.apply(&self.sample)).mono().muted());
		vec![
			view::column()
				.gap(Space::Lg)
				.fill()
				.child(tester)
				.child(self.section(Kind::Simple, "Simple replacements"))
				.child(self.section(Kind::Regex, "Regex replacements")),
		]
	}
}

impl Plugin for TextReplace {
	fn init(&mut self) -> Result<(), String> {
		self.simple = load_rules(STRING_RULES_KEY);
		self.regex = load_rules(REGEX_RULES_KEY);
		self.recompile();
		let options = mount::Options::new("Rules");
		if let Err(error) = mount::mount(mount::SETTINGS_PAGE, MOUNT_KEY, self.page(), options) {
			log::warn!("could not mount the rules page: {error:?}");
		}
		Ok(())
	}

	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		let content = self.apply(&msg.content);
		if content == msg.content || content.is_empty() || content.len() > MAX_CONTENT_BYTES {
			SendDecision::Pass
		} else {
			SendDecision::Replace(OutgoingMessage { content, ..msg })
		}
	}
}

impl PluginExt for TextReplace {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.point != SETTINGS_POINT {
			return Reply::none();
		}
		let node = event.node.as_str();
		match &event.kind {
			ViewEventKind::Change(value) if node == TEST_FIELD => self.sample = clip(value),
			ViewEventKind::Change(value) => {
				let mut parts = node.splitn(3, ':');
				if let (Some(kind), Some(index), Some(field)) = (
					parts.next().and_then(Kind::from_tag),
					parts.next().and_then(|index| index.parse().ok()),
					parts.next(),
				) {
					self.edit(kind, index, field, value);
				}
			}
			ViewEventKind::Click => {
				if let Some(kind) = node.strip_prefix(ADD_PREFIX).and_then(Kind::from_tag) {
					if self.rules(kind).len() < MAX_RULES {
						self.rules_mut(kind).push(Rule::default());
						self.changed(kind);
					}
				} else if let Some((kind, index)) = node
					.strip_prefix(DELETE_PREFIX)
					.and_then(|rest| rest.split_once(':'))
					.and_then(|(kind, index)| Some((Kind::from_tag(kind)?, index.parse::<usize>().ok()?)))
					&& index < self.rules(kind).len()
				{
					self.rules_mut(kind).remove(index);
					self.changed(kind);
				}
			}
			_ => return Reply::none(),
		}
		Reply::none().replace(self.page())
	}
}

export_plugin!(TextReplace, ext);
