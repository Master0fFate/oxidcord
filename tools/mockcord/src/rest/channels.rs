use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::Arc;

use axum::Json;
use axum::extract::{FromRequest, Multipart, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use protocol::{ChannelType, EditMessage, Snowflake};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{ApiError, ApiResult};
use crate::fixtures::CURRENT_USER_ID;
use crate::json::{message_id, parse_id, to_json};
use crate::ops::{Draft, UploadedFile};
use crate::state::{Shared, Store};
use crate::threads::ThreadRequest;

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 100;
const DEFAULT_SEARCH_PAGE_SIZE: usize = 25;
const DEFAULT_ARCHIVE_MINUTES: i64 = 1440;

#[derive(Deserialize, Default)]
pub struct MessagesParams {
	limit: Option<usize>,
	before: Option<u64>,
	after: Option<u64>,
	around: Option<u64>,
}

fn page(messages: &BTreeMap<u64, Value>, params: &MessagesParams) -> Vec<Value> {
	let limit = params.limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
	if let Some(around) = params.around {
		let older = limit / 2;
		let target = messages.get(&around);
		let newer_count = limit - older - usize::from(target.is_some());
		let newer: Vec<&Value> = messages
			.range((Excluded(around), Unbounded))
			.take(newer_count)
			.map(|(_, message)| message)
			.collect();
		let mut result: Vec<Value> = newer.into_iter().rev().cloned().collect();
		result.extend(target.cloned());
		result.extend(
			messages
				.range(..around)
				.rev()
				.take(older)
				.map(|(_, message)| message.clone()),
		);
		result
	} else if let Some(before) = params.before {
		messages
			.range(..before)
			.rev()
			.take(limit)
			.map(|(_, message)| message.clone())
			.collect()
	} else if let Some(after) = params.after {
		let mut result: Vec<Value> = messages
			.range((Excluded(after), Unbounded))
			.take(limit)
			.map(|(_, message)| message.clone())
			.collect();
		result.reverse();
		result
	} else {
		messages.values().rev().take(limit).cloned().collect()
	}
}

fn require_channel(store: &Store, id: u64) -> Result<(), ApiError> {
	if store.channels.contains_key(&id) {
		Ok(())
	} else {
		Err(ApiError::unknown_channel())
	}
}

pub async fn channel(State(shared): State<Arc<Shared>>, Path(id): Path<u64>) -> ApiResult {
	shared
		.store
		.lock()
		.channels
		.get(&id)
		.map(|channel| Json(to_json(channel)))
		.ok_or_else(ApiError::unknown_channel)
}

pub async fn list_messages(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
	Query(params): Query<MessagesParams>,
) -> ApiResult {
	let store = shared.store.lock();
	require_channel(&store, id)?;
	let messages = store
		.messages
		.get(&id)
		.map(|messages| page(messages, &params))
		.unwrap_or_default();
	Ok(Json(Value::Array(messages)))
}

pub async fn message(State(shared): State<Arc<Shared>>, Path((id, mid)): Path<(u64, u64)>) -> ApiResult {
	let store = shared.store.lock();
	require_channel(&store, id)?;
	store
		.message(id, mid)
		.map(|message| Json(message.clone()))
		.ok_or_else(ApiError::unknown_message)
}

fn multipart_error<E>(_: E) -> ApiError {
	ApiError::invalid_body("Invalid multipart body")
}

fn draft_from_json(mut body: Value, files: Vec<UploadedFile>) -> Result<Draft, ApiError> {
	if body["nonce"].is_number() {
		body["nonce"] = Value::String(body["nonce"].to_string());
	}
	let create = serde_json::from_value(body).map_err(|_| ApiError::invalid_body("Invalid Form Body"))?;
	Ok(Draft::from_create(create, files))
}

async fn read_multipart_parts(request: Request) -> Result<(Value, Vec<UploadedFile>), ApiError> {
	let mut form = Multipart::from_request(request, &())
		.await
		.map_err(multipart_error)?;
	let mut payload = json!({});
	let mut files = Vec::new();
	while let Some(field) = form.next_field().await.map_err(multipart_error)? {
		let name = field.name().unwrap_or_default().to_owned();
		if name == "payload_json" {
			let text = field.text().await.map_err(multipart_error)?;
			payload = serde_json::from_str(&text).map_err(multipart_error)?;
		} else if name.starts_with("files[") {
			let filename = field.file_name().unwrap_or("file").to_owned();
			let content_type = field.content_type().map(str::to_owned);
			let bytes = field.bytes().await.map_err(multipart_error)?;
			files.push(UploadedFile {
				filename,
				content_type,
				size: bytes.len(),
			});
		}
	}
	Ok((payload, files))
}

async fn read_body(request: Request) -> Result<(Value, Vec<UploadedFile>), ApiError> {
	let multipart = request
		.headers()
		.get(CONTENT_TYPE)
		.and_then(|value| value.to_str().ok())
		.is_some_and(|value| value.starts_with("multipart/form-data"));
	if multipart {
		return read_multipart_parts(request).await;
	}
	let Json(body) = Json::<Value>::from_request(request, &())
		.await
		.map_err(|_| ApiError::invalid_body("Invalid JSON body"))?;
	Ok((body, Vec::new()))
}

async fn read_draft(request: Request) -> Result<Draft, ApiError> {
	let (body, files) = read_body(request).await?;
	draft_from_json(body, files)
}

pub async fn create_message(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
	request: Request,
) -> ApiResult {
	let draft = read_draft(request).await?;
	shared
		.post_message(id, &shared.fixtures.current_user, draft)
		.map(Json)
}

pub async fn edit_message(
	State(shared): State<Arc<Shared>>,
	Path((id, mid)): Path<(u64, u64)>,
	Json(edit): Json<EditMessage>,
) -> ApiResult {
	shared.edit_message(id, mid, edit).map(Json)
}

pub async fn delete_message(
	State(shared): State<Arc<Shared>>,
	Path((id, mid)): Path<(u64, u64)>,
) -> Result<StatusCode, ApiError> {
	shared.delete_message(id, mid)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn add_reaction(
	State(shared): State<Arc<Shared>>,
	Path((id, mid, emoji)): Path<(u64, u64, String)>,
) -> Result<StatusCode, ApiError> {
	shared.set_reaction(id, mid, &emoji, true)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_reaction(
	State(shared): State<Arc<Shared>>,
	Path((id, mid, emoji)): Path<(u64, u64, String)>,
) -> Result<StatusCode, ApiError> {
	shared.set_reaction(id, mid, &emoji, false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn pins(State(shared): State<Arc<Shared>>, Path(id): Path<u64>) -> ApiResult {
	let store = shared.store.lock();
	require_channel(&store, id)?;
	let pinned: Vec<Value> = store
		.messages
		.get(&id)
		.map(|messages| {
			messages
				.values()
				.rev()
				.filter(|message| message["pinned"].as_bool() == Some(true))
				.cloned()
				.collect()
		})
		.unwrap_or_default();
	Ok(Json(Value::Array(pinned)))
}

pub async fn typing(State(shared): State<Arc<Shared>>, Path(id): Path<u64>) -> Result<StatusCode, ApiError> {
	shared.typing(id, CURRENT_USER_ID)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn call_signal(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
) -> Result<StatusCode, ApiError> {
	require_channel(&shared.store.lock(), id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn ack(
	State(shared): State<Arc<Shared>>,
	Path((id, mid)): Path<(u64, u64)>,
	body: Option<Json<Value>>,
) -> ApiResult {
	let mention_count = body.and_then(|Json(body)| body["mention_count"].as_i64());
	shared.ack(id, mid, mention_count)?;
	Ok(Json(json!({ "token": null })))
}

pub fn search_in(store: &Store, channel_ids: &[u64], params: &HashMap<String, String>) -> Value {
	let content = params.get("content").map(|content| content.to_lowercase());
	let author = params
		.get("author_id")
		.and_then(|author| author.parse::<u64>().ok());
	let only_channel = params
		.get("channel_id")
		.and_then(|channel| channel.parse::<u64>().ok());
	let mut hits: Vec<&Value> = channel_ids
		.iter()
		.filter(|id| only_channel.is_none_or(|only| only == **id))
		.filter_map(|id| store.messages.get(id))
		.flat_map(BTreeMap::values)
		.filter(|message| {
			content.as_ref().is_none_or(|needle| {
				message["content"]
					.as_str()
					.is_some_and(|text| text.to_lowercase().contains(needle.as_str()))
			}) && author.is_none_or(|author| parse_id(&message["author"]["id"]) == Some(author))
		})
		.collect();
	hits.sort_by_key(|message| Reverse(message_id(message)));
	let total = hits.len();
	let offset = params
		.get("offset")
		.and_then(|offset| offset.parse().ok())
		.unwrap_or(0);
	let limit = params
		.get("limit")
		.and_then(|limit| limit.parse().ok())
		.unwrap_or(DEFAULT_SEARCH_PAGE_SIZE)
		.clamp(1, DEFAULT_SEARCH_PAGE_SIZE);
	let messages: Vec<Value> = hits
		.into_iter()
		.skip(offset)
		.take(limit)
		.map(|message| {
			let mut hit = message.clone();
			hit["hit"] = Value::Bool(true);
			Value::Array(vec![hit])
		})
		.collect();
	json!({ "total_results": total, "messages": messages, "analytics_id": "mock" })
}

pub async fn search_messages(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
	Query(params): Query<HashMap<String, String>>,
) -> ApiResult {
	let store = shared.store.lock();
	require_channel(&store, id)?;
	Ok(Json(search_in(&store, &[id], &params)))
}

fn thread_request(body: &Value, starter: Option<Draft>) -> Result<ThreadRequest, ApiError> {
	let name = body["name"]
		.as_str()
		.filter(|name| !name.is_empty())
		.ok_or_else(|| ApiError::invalid_body("name is required"))?;
	Ok(ThreadRequest {
		name: name.to_owned(),
		auto_archive_duration: body["auto_archive_duration"]
			.as_i64()
			.unwrap_or(DEFAULT_ARCHIVE_MINUTES),
		kind: ChannelType::from(body["type"].as_i64().unwrap_or(11)),
		applied_tags: body["applied_tags"]
			.as_array()
			.map(|tags| tags.iter().filter_map(parse_id).map(Snowflake).collect())
			.unwrap_or_default(),
		starter,
	})
}

pub async fn create_thread(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
	request: Request,
) -> ApiResult {
	let (body, files) = read_body(request).await?;
	let starter = match body.get("message") {
		Some(message) => Some(draft_from_json(message.clone(), files)?),
		None => None,
	};
	shared
		.create_thread(id, None, thread_request(&body, starter)?)
		.map(Json)
}

pub async fn thread_from_message(
	State(shared): State<Arc<Shared>>,
	Path((id, mid)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> ApiResult {
	shared
		.create_thread(id, Some(mid), thread_request(&body, None)?)
		.map(Json)
}
