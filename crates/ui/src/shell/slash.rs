use protocol::{ApplicationCommand, ApplicationCommandOption, CommandOptionType, InteractionOption};
use rest::UploadFile;
use serde_json::{Number, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Invalid {
	Number,
	Range,
	Length,
}

#[derive(Clone, Default)]
pub(super) struct Slot {
	pub text: String,
	pub value: Option<Value>,
	pub file: Option<UploadFile>,
}

impl Slot {
	fn filled(&self) -> bool {
		self.value.is_some() || self.file.is_some()
	}
}

#[derive(Clone)]
pub(super) struct SlashDraft {
	command: Box<ApplicationCommand>,
	path: Vec<usize>,
	pub slots: Vec<Slot>,
	pub focused: usize,
}

pub(super) fn is_typed(option: &ApplicationCommandOption) -> bool {
	option.choices.is_empty()
		&& matches!(
			option.kind,
			CommandOptionType::String | CommandOptionType::Integer | CommandOptionType::Number
		)
}

fn options_at<'a>(command: &'a ApplicationCommand, path: &[usize]) -> &'a [ApplicationCommandOption] {
	let mut options = command.options.as_slice();
	for &index in path {
		match options.get(index) {
			Some(option) => options = option.options.as_slice(),
			None => break,
		}
	}
	options
}

fn collect_paths(options: &[ApplicationCommandOption], path: &mut Vec<usize>, found: &mut Vec<Vec<usize>>) {
	if !options.iter().any(|option| option.kind.is_group()) {
		found.push(path.clone());
		return;
	}
	for (index, option) in options
		.iter()
		.enumerate()
		.filter(|(_, option)| option.kind.is_group())
	{
		path.push(index);
		collect_paths(&option.options, path, found);
		path.pop();
	}
}

fn bounded(value: f64, min: Option<&Number>, max: Option<&Number>) -> bool {
	min.and_then(Number::as_f64).is_none_or(|min| value >= min)
		&& max.and_then(Number::as_f64).is_none_or(|max| value <= max)
}

fn parse(option: &ApplicationCommandOption, text: &str) -> Result<Value, Invalid> {
	let in_range = |number: f64| {
		bounded(number, option.min_value.as_ref(), option.max_value.as_ref())
			.then_some(())
			.ok_or(Invalid::Range)
	};
	match option.kind {
		CommandOptionType::Integer => {
			let number: i64 = text.parse().map_err(|_| Invalid::Number)?;
			in_range(number as f64)?;
			Ok(Value::from(number))
		}
		CommandOptionType::Number => {
			let number: f64 = text
				.parse()
				.ok()
				.filter(|number: &f64| number.is_finite())
				.ok_or(Invalid::Number)?;
			in_range(number)?;
			Number::from_f64(number).map(Value::Number).ok_or(Invalid::Number)
		}
		_ => {
			let length = text.chars().count() as f64;
			let min = option.min_length.map(Number::from);
			let max = option.max_length.map(Number::from);
			bounded(length, min.as_ref(), max.as_ref())
				.then(|| Value::String(text.to_owned()))
				.ok_or(Invalid::Length)
		}
	}
}

pub(super) fn command_paths(command: &ApplicationCommand) -> Vec<Vec<usize>> {
	let mut paths = Vec::new();
	collect_paths(&command.options, &mut Vec::new(), &mut paths);
	paths
}

fn chain_at<'a>(
	command: &'a ApplicationCommand,
	path: &'a [usize],
) -> impl Iterator<Item = &'a ApplicationCommandOption> {
	let mut options = command.options.as_slice();
	path.iter().map_while(move |&index| {
		let option = options.get(index)?;
		options = option.options.as_slice();
		Some(option)
	})
}

pub(super) fn label_at(command: &ApplicationCommand, path: &[usize]) -> String {
	chain_at(command, path).fold(command.name.clone(), |label, option| {
		format!("{label} {}", option.name)
	})
}

impl SlashDraft {
	pub fn at(command: &ApplicationCommand, path: Vec<usize>) -> Self {
		let slots = vec![Slot::default(); options_at(command, &path).len()];
		Self {
			command: Box::new(command.clone()),
			path,
			slots,
			focused: 0,
		}
	}

	fn chain(&self) -> impl Iterator<Item = &ApplicationCommandOption> {
		chain_at(&self.command, &self.path)
	}

	pub fn command(&self) -> &ApplicationCommand {
		&self.command
	}

	pub fn options(&self) -> &[ApplicationCommandOption] {
		options_at(&self.command, &self.path)
	}

	pub fn focused_option(&self) -> Option<&ApplicationCommandOption> {
		self.options().get(self.focused)
	}

	pub fn label(&self) -> String {
		label_at(&self.command, &self.path)
	}

	pub fn description(&self) -> Option<&str> {
		self.chain()
			.last()
			.and_then(|option| option.description.as_deref())
			.or(self.command.description.as_deref())
			.filter(|description| !description.is_empty())
	}

	pub fn type_text(&mut self, text: &str) {
		let typed = self.focused_option().is_some_and(is_typed);
		if let Some(slot) = self
			.slots
			.get_mut(self.focused)
			.filter(|slot| typed && slot.text != text)
		{
			text.clone_into(&mut slot.text);
			slot.value = None;
		}
	}

	pub fn focused_filled(&self) -> bool {
		self.slots.get(self.focused).is_some_and(Slot::filled)
	}

	pub fn fill(&mut self, value: Value, text: String) {
		if let Some(slot) = self.slots.get_mut(self.focused) {
			*slot = Slot {
				text,
				value: Some(value),
				file: None,
			};
		}
	}

	pub fn attach(&mut self, file: UploadFile) {
		if let Some(slot) = self.slots.get_mut(self.focused) {
			*slot = Slot {
				text: file.name.clone(),
				value: None,
				file: Some(file),
			};
		}
	}

	pub fn commit(&mut self) -> Result<(), Invalid> {
		let Some(option) = options_at(&self.command, &self.path).get(self.focused) else {
			return Ok(());
		};
		let Some(slot) = self.slots.get_mut(self.focused) else {
			return Ok(());
		};
		if !is_typed(option) || slot.value.is_some() {
			return Ok(());
		}
		let text = slot.text.trim();
		if !text.is_empty() {
			slot.value = Some(parse(option, text)?);
		}
		Ok(())
	}

	pub fn missing(&self) -> Option<usize> {
		self.options()
			.iter()
			.zip(&self.slots)
			.position(|(option, slot)| option.required && !slot.filled())
	}

	fn leaf_options(&self, files: &mut Vec<UploadFile>) -> Vec<InteractionOption> {
		self.options()
			.iter()
			.zip(&self.slots)
			.filter_map(|(option, slot)| {
				let value = match (&slot.value, &slot.file) {
					(Some(value), _) => value.clone(),
					(None, Some(file)) => {
						files.push(file.clone());
						Value::String((files.len() - 1).to_string())
					}
					(None, None) => return None,
				};
				Some(InteractionOption {
					kind: option.kind,
					name: option.name.clone(),
					value: Some(value),
					options: Vec::new(),
					focused: None,
				})
			})
			.collect()
	}

	fn nested(&self, leaf: Vec<InteractionOption>) -> Vec<InteractionOption> {
		let chain: Vec<&ApplicationCommandOption> = self.chain().collect();
		chain.into_iter().rev().fold(leaf, |options, group| {
			vec![InteractionOption {
				kind: group.kind,
				name: group.name.clone(),
				value: None,
				options,
				focused: None,
			}]
		})
	}

	pub fn partial(&self, text: &str) -> Vec<InteractionOption> {
		let leaf = self
			.options()
			.iter()
			.zip(&self.slots)
			.enumerate()
			.filter_map(|(index, (option, slot))| {
				let value = if index == self.focused {
					Value::String(text.to_owned())
				} else {
					slot.value.clone()?
				};
				Some(InteractionOption {
					kind: option.kind,
					name: option.name.clone(),
					value: Some(value),
					options: Vec::new(),
					focused: (index == self.focused).then_some(true),
				})
			})
			.collect();
		self.nested(leaf)
	}

	pub fn finish(self) -> (Box<ApplicationCommand>, Vec<InteractionOption>, Vec<UploadFile>) {
		let mut files = Vec::new();
		let leaf = self.leaf_options(&mut files);
		let options = self.nested(leaf);
		(self.command, options, files)
	}
}
