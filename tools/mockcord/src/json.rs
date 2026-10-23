use serde::Serialize;
use serde_json::Value;

pub fn to_json<T: Serialize + ?Sized>(value: &T) -> Value {
	serde_json::to_value(value).unwrap_or(Value::Null)
}

pub fn parse_id(value: &Value) -> Option<u64> {
	match value {
		Value::String(text) => text.parse().ok(),
		Value::Number(number) => number.as_u64(),
		_ => None,
	}
}

pub fn message_id(message: &Value) -> u64 {
	parse_id(&message["id"]).unwrap_or_default()
}

pub fn author_id(message: &Value) -> u64 {
	parse_id(&message["author"]["id"]).unwrap_or_default()
}

pub fn mentions_user(message: &Value, user_id: u64) -> bool {
	message["mentions"]
		.as_array()
		.is_some_and(|mentions| mentions.iter().any(|user| parse_id(&user["id"]) == Some(user_id)))
}
