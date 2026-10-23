use crate::data::Data;

pub struct CommandHint {
	pub name: String,
	pub description: String,
}

pub fn list(data: &Data) -> Vec<CommandHint> {
	data.session()
		.plugin_commands()
		.into_iter()
		.map(|(name, entry)| CommandHint {
			name,
			description: entry.description,
		})
		.collect()
}

pub fn parse(data: &Data, text: &str) -> Option<(String, String)> {
	let body = text.strip_prefix('/')?;
	let (name, args) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
	data.session()
		.plugin_commands()
		.iter()
		.any(|(known, _)| known == name)
		.then(|| (name.to_owned(), args.trim().to_owned()))
}
