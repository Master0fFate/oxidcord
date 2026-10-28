use std::sync::Arc;

use plugin_host::CommandEntry;

use crate::data::Data;

pub fn list(data: &Data) -> Arc<[(String, CommandEntry)]> {
	data.session().plugin_commands()
}

pub fn parse(data: &Data, text: &str) -> Option<(String, String)> {
	let body = text.strip_prefix('/')?;
	let (name, args) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
	data.session()
		.plugin_commands()
		.binary_search_by(|(known, _)| known.as_str().cmp(name))
		.is_ok()
		.then(|| (name.to_owned(), args.trim().to_owned()))
}
