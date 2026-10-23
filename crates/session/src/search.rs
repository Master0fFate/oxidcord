use protocol::Message;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
	pub messages: Vec<Message>,
	pub total: u64,
}

fn hit_of(group: &Value) -> Option<&Value> {
	let entries = group.as_array()?;
	entries
		.iter()
		.find(|entry| entry["hit"].as_bool() == Some(true))
		.or_else(|| entries.first())
}

impl SearchResults {
	pub fn from_response(response: &Value) -> Self {
		let messages: Vec<Message> = response["messages"]
			.as_array()
			.into_iter()
			.flatten()
			.filter_map(hit_of)
			.filter_map(|entry| serde_json::from_value(entry.clone()).ok())
			.collect();
		let total = response["total_results"]
			.as_u64()
			.unwrap_or(messages.len() as u64);
		Self { messages, total }
	}
}
