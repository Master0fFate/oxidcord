use chrono::Utc;
use serde_json::{Map, Value, json};

use crate::apps::{Apps, RpcApp};

const TEXT_MIN: usize = 2;
const TEXT_MAX: usize = 128;
const URL_MAX: usize = 256;
const PARTY_ID_MAX: usize = 128;
const BUTTON_LABEL_MAX: usize = 32;
const BUTTON_URL_MAX: usize = 512;
const MAX_BUTTONS: usize = 2;
const SECONDS_CUTOFF: i64 = 100_000_000_000;
const ALLOWED_TYPES: [i64; 4] = [0, 2, 3, 5];
const PASSTHROUGH_IMAGE_PREFIXES: [&str; 4] = ["mp:", "spotify:", "youtube:", "twitch:"];

fn fail(path: &[&str], reason: &str) -> String {
	let mut message = format!("\"{}\" {reason}", path[path.len() - 1]);
	for field in path[..path.len() - 1].iter().rev() {
		message = format!("child \"{field}\" fails because [{message}]");
	}
	format!("child \"activity\" fails because [{message}]")
}

fn text(container: &Value, path: &[&str], min: usize, max: usize) -> Result<Option<String>, String> {
	let Some(value) = container.get(path[path.len() - 1]) else {
		return Ok(None);
	};
	if value.is_null() {
		return Ok(None);
	}
	let Some(value) = value.as_str() else {
		return Err(fail(path, "must be a string"));
	};
	let length = value.chars().count();
	if length < min {
		return Err(fail(
			path,
			&format!("length must be at least {min} characters long"),
		));
	}
	if length > max {
		return Err(fail(
			path,
			&format!("length must be less than or equal to {max} characters long"),
		));
	}
	Ok(Some(value.to_owned()))
}

fn put(target: &mut Map<String, Value>, key: &str, value: Option<impl Into<Value>>) {
	if let Some(value) = value {
		target.insert(key.to_owned(), value.into());
	}
}

fn timestamp(container: &Value, path: &[&str]) -> Result<Option<i64>, String> {
	let Some(value) = container.get(path[path.len() - 1]) else {
		return Ok(None);
	};
	if value.is_null() {
		return Ok(None);
	}
	let Some(number) = value
		.as_i64()
		.or_else(|| value.as_f64().map(|float| float as i64))
	else {
		return Err(fail(path, "must be a number"));
	};
	Ok(Some(if number < SECONDS_CUTOFF {
		number * 1000
	} else {
		number
	}))
}

async fn image(app: &RpcApp, apps: &Apps, key: &str) -> Option<String> {
	if key.is_empty() {
		return None;
	}
	if PASSTHROUGH_IMAGE_PREFIXES
		.iter()
		.any(|prefix| key.starts_with(prefix))
		|| key.bytes().all(|byte| byte.is_ascii_digit())
	{
		return Some(key.to_owned());
	}
	if key.starts_with("https://") || key.starts_with("http://") {
		return apps.external(app.id.get(), key).await;
	}
	Some(
		apps.asset_id(app.id.get(), key)
			.await
			.unwrap_or_else(|| key.to_owned()),
	)
}

async fn assets(app: &RpcApp, apps: &Apps, raw: &Value) -> Result<Option<Value>, String> {
	let Some(raw) = raw.get("assets").filter(|assets| !assets.is_null()) else {
		return Ok(None);
	};
	if !raw.is_object() {
		return Err(fail(&["assets"], "must be an object"));
	}
	let mut assets = Map::new();
	if let Some(key) = text(raw, &["assets", "large_image"], 1, URL_MAX)? {
		put(&mut assets, "large_image", image(app, apps, &key).await);
	}
	put(
		&mut assets,
		"large_text",
		text(raw, &["assets", "large_text"], TEXT_MIN, TEXT_MAX)?,
	);
	put(
		&mut assets,
		"large_url",
		text(raw, &["assets", "large_url"], 1, URL_MAX)?,
	);
	if let Some(key) = text(raw, &["assets", "small_image"], 1, URL_MAX)? {
		put(&mut assets, "small_image", image(app, apps, &key).await);
	}
	put(
		&mut assets,
		"small_text",
		text(raw, &["assets", "small_text"], TEXT_MIN, TEXT_MAX)?,
	);
	put(
		&mut assets,
		"small_url",
		text(raw, &["assets", "small_url"], 1, URL_MAX)?,
	);
	Ok((!assets.is_empty()).then_some(Value::Object(assets)))
}

fn party(raw: &Value) -> Result<Option<Value>, String> {
	let Some(raw) = raw.get("party").filter(|party| !party.is_null()) else {
		return Ok(None);
	};
	if !raw.is_object() {
		return Err(fail(&["party"], "must be an object"));
	}
	let mut party = Map::new();
	put(
		&mut party,
		"id",
		text(raw, &["party", "id"], TEXT_MIN, PARTY_ID_MAX)?,
	);
	if let Some(size) = raw.get("size").filter(|size| !size.is_null()) {
		let sizes: Option<Vec<i64>> = size
			.as_array()
			.filter(|sizes| sizes.len() == 2)
			.map(|sizes| sizes.iter().filter_map(Value::as_i64).collect())
			.filter(|sizes: &Vec<i64>| sizes.len() == 2);
		let Some(sizes) = sizes else {
			return Err(fail(&["party", "size"], "must be an array of two numbers"));
		};
		party.insert("size".to_owned(), json!(sizes));
	}
	Ok((!party.is_empty()).then_some(Value::Object(party)))
}

fn secrets(raw: &Value) -> Result<Option<Value>, String> {
	let Some(raw) = raw.get("secrets").filter(|secrets| !secrets.is_null()) else {
		return Ok(None);
	};
	if !raw.is_object() {
		return Err(fail(&["secrets"], "must be an object"));
	}
	let mut secrets = Map::new();
	put(
		&mut secrets,
		"join",
		text(raw, &["secrets", "join"], 1, TEXT_MAX)?,
	);
	put(
		&mut secrets,
		"spectate",
		text(raw, &["secrets", "spectate"], 1, TEXT_MAX)?,
	);
	put(
		&mut secrets,
		"match",
		text(raw, &["secrets", "match"], 1, TEXT_MAX)?,
	);
	Ok((!secrets.is_empty()).then_some(Value::Object(secrets)))
}

struct Buttons {
	labels: Vec<String>,
	urls: Vec<String>,
}

fn buttons(raw: &Value) -> Result<Option<Buttons>, String> {
	let Some(raw) = raw.get("buttons").filter(|buttons| !buttons.is_null()) else {
		return Ok(None);
	};
	let Some(entries) = raw.as_array() else {
		return Err(fail(&["buttons"], "must be an array"));
	};
	if entries.is_empty() {
		return Ok(None);
	}
	if entries.len() > MAX_BUTTONS {
		return Err(fail(
			&["buttons"],
			&format!("must contain less than or equal to {MAX_BUTTONS} items"),
		));
	}
	let mut labels = Vec::with_capacity(entries.len());
	let mut urls = Vec::with_capacity(entries.len());
	for (index, entry) in entries.iter().enumerate() {
		let position = index.to_string();
		let label = text(entry, &["buttons", &position, "label"], 1, BUTTON_LABEL_MAX)?
			.ok_or_else(|| fail(&["buttons", &position, "label"], "is required"))?;
		let url = text(entry, &["buttons", &position, "url"], 1, BUTTON_URL_MAX)?
			.ok_or_else(|| fail(&["buttons", &position, "url"], "is required"))?;
		labels.push(label);
		urls.push(url);
	}
	Ok(Some(Buttons { labels, urls }))
}

pub(crate) async fn build(app: &RpcApp, apps: &Apps, raw: &Value) -> Result<Value, String> {
	if !raw.is_object() {
		return Err(r#"child "activity" fails because ["activity" must be an object]"#.to_owned());
	}
	let kind = match raw.get("type") {
		None | Some(Value::Null) => 0,
		Some(value) => value
			.as_i64()
			.filter(|kind| ALLOWED_TYPES.contains(kind))
			.ok_or_else(|| fail(&["type"], "must be one of [0, 2, 3, 5]"))?,
	};
	let mut activity = Map::new();
	activity.insert("name".to_owned(), Value::String(app.name.clone()));
	activity.insert("type".to_owned(), Value::from(kind));
	activity.insert("application_id".to_owned(), Value::String(app.id.to_string()));
	put(
		&mut activity,
		"details",
		text(raw, &["details"], TEXT_MIN, TEXT_MAX)?,
	);
	put(
		&mut activity,
		"details_url",
		text(raw, &["details_url"], 1, URL_MAX)?,
	);
	put(&mut activity, "state", text(raw, &["state"], TEXT_MIN, TEXT_MAX)?);
	put(&mut activity, "state_url", text(raw, &["state_url"], 1, URL_MAX)?);
	if let Some(timestamps) = raw.get("timestamps").filter(|timestamps| !timestamps.is_null()) {
		if !timestamps.is_object() {
			return Err(fail(&["timestamps"], "must be an object"));
		}
		let mut stamps = Map::new();
		put(
			&mut stamps,
			"start",
			timestamp(timestamps, &["timestamps", "start"])?,
		);
		put(&mut stamps, "end", timestamp(timestamps, &["timestamps", "end"])?);
		if !stamps.is_empty() {
			activity.insert("timestamps".to_owned(), Value::Object(stamps));
		}
	}
	put(&mut activity, "assets", assets(app, apps, raw).await?);
	put(&mut activity, "party", party(raw)?);
	let secrets = secrets(raw)?;
	let buttons = buttons(raw)?;
	if secrets.is_some() && buttons.is_some() {
		return Err(fail(&["buttons"], "cannot be used together with secrets"));
	}
	put(&mut activity, "secrets", secrets);
	if let Some(Buttons { labels, urls }) = buttons {
		activity.insert("buttons".to_owned(), json!(labels));
		activity.insert("metadata".to_owned(), json!({ "button_urls": urls }));
	}
	if let Some(instance) = raw.get("instance").and_then(Value::as_bool) {
		activity.insert("instance".to_owned(), Value::Bool(instance));
	}
	if let Some(display) = raw.get("status_display_type").and_then(Value::as_i64) {
		activity.insert("status_display_type".to_owned(), Value::from(display));
	}
	activity.insert(
		"created_at".to_owned(),
		Value::from(Utc::now().timestamp_millis()),
	);
	Ok(Value::Object(activity))
}
