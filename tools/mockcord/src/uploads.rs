use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use futures_util::StreamExt;
use protocol::{AttachmentDescriptor, PremiumType};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{ApiError, ApiResult};
use crate::json::to_json;
use crate::ops::{UploadedFile, sanitized_filename};
use crate::state::Shared;

const BASE_UPLOAD_BYTES: u64 = 20 * 1024 * 1024;
const PREMIUM_UPLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const BASE_MESSAGE_CHARS: usize = 2000;
const PREMIUM_MESSAGE_CHARS: usize = 4000;
const SLOWMODE_RETRY_SECONDS: f64 = 5.0;
const CHUNK_PAUSE: Duration = Duration::from_millis(2);
const API_PREFIX: &str = "/api/v10";

#[derive(Clone, Copy)]
pub enum SendFault {
	Server,
	Slowmode,
	Permission,
	Automod,
	SkipEcho,
}

impl SendFault {
	fn parse(name: &str) -> Option<Self> {
		match name {
			"server" => Some(Self::Server),
			"slowmode" => Some(Self::Slowmode),
			"permission" => Some(Self::Permission),
			"automod" => Some(Self::Automod),
			"echo" => Some(Self::SkipEcho),
			_ => None,
		}
	}

	pub fn error(self) -> Option<ApiError> {
		match self {
			Self::Server => Some(ApiError::server_error()),
			Self::Slowmode => Some(ApiError::slowmode(SLOWMODE_RETRY_SECONDS)),
			Self::Permission => Some(ApiError::missing_permissions()),
			Self::Automod => Some(ApiError::automod_blocked()),
			Self::SkipEcho => None,
		}
	}
}

#[derive(Deserialize)]
pub struct SlotRequest {
	files: Vec<SlotFile>,
}

#[derive(Deserialize)]
struct SlotFile {
	#[serde(default)]
	id: Value,
	filename: String,
	file_size: u64,
}

impl Shared {
	pub fn take_send_fault(&self) -> Option<SendFault> {
		self.send_fault.lock().take()
	}

	pub async fn pause_upload(&self) {
		if self.slow_uploads.load(Ordering::Relaxed) {
			tokio::time::sleep(CHUNK_PAUSE).await;
		}
	}

	fn has_premium(&self) -> bool {
		self.store
			.lock()
			.current_user
			.premium_type
			.is_some_and(|kind| kind != PremiumType::None)
	}

	fn upload_limit(&self) -> u64 {
		if self.has_premium() {
			PREMIUM_UPLOAD_BYTES
		} else {
			BASE_UPLOAD_BYTES
		}
	}

	pub fn check_limits(&self, content: &str, files: &[UploadedFile]) -> Result<(), ApiError> {
		let message_limit = if self.has_premium() {
			PREMIUM_MESSAGE_CHARS
		} else {
			BASE_MESSAGE_CHARS
		};
		if content.chars().count() > message_limit {
			return Err(ApiError::invalid_body(
				"content: Must be 2000 or fewer in length.",
			));
		}
		let limit = self.upload_limit();
		if files.iter().any(|file| file.size as u64 > limit) {
			return Err(ApiError::file_too_large());
		}
		Ok(())
	}

	pub fn claim_uploads(&self, descriptors: &[AttachmentDescriptor]) -> Result<Vec<UploadedFile>, ApiError> {
		let mut store = self.store.lock();
		descriptors
			.iter()
			.filter_map(|descriptor| {
				descriptor
					.uploaded_filename
					.as_deref()
					.map(|name| (descriptor, name))
			})
			.map(|(descriptor, name)| {
				let size = store
					.uploads
					.remove(name)
					.ok_or_else(|| ApiError::invalid_body("Upload was not completed"))?;
				Ok(UploadedFile {
					filename: descriptor.filename.clone().unwrap_or_else(|| name.to_owned()),
					content_type: None,
					size: size as usize,
				})
			})
			.collect()
	}

	fn set_premium(&self, kind: PremiumType) {
		let value = {
			let mut store = self.store.lock();
			store.current_user.premium_type = Some(kind);
			to_json(&store.current_user)
		};
		self.hub.broadcast("USER_UPDATE", &value);
	}
}

pub async fn upload_slots(
	State(shared): State<Arc<Shared>>,
	Path(channel_id): Path<u64>,
	Json(request): Json<SlotRequest>,
) -> ApiResult {
	if !shared.store.lock().channels.contains_key(&channel_id) {
		return Err(ApiError::unknown_channel());
	}
	let limit = shared.upload_limit();
	if request.files.iter().any(|file| file.file_size > limit) {
		return Err(ApiError::file_too_large());
	}
	let origin = shared.urls.rest.trim_end_matches(API_PREFIX);
	let mut store = shared.store.lock();
	let attachments: Vec<Value> = request
		.files
		.iter()
		.map(|file| {
			let upload_filename = format!(
				"{}/{}",
				store.next_snowflake(),
				sanitized_filename(&file.filename)
			);
			json!({
				"id": file.id,
				"upload_url": format!("{origin}/mock/upload/{upload_filename}"),
				"upload_filename": upload_filename,
			})
		})
		.collect();
	Ok(Json(json!({ "attachments": attachments })))
}

pub async fn put_upload(
	State(shared): State<Arc<Shared>>,
	Path((token, name)): Path<(String, String)>,
	body: Body,
) -> Result<StatusCode, ApiError> {
	let mut chunks = body.into_data_stream();
	let mut size = 0u64;
	while let Some(chunk) = chunks.next().await {
		size += chunk
			.map_err(|_| ApiError::invalid_body("Upload was interrupted"))?
			.len() as u64;
		shared.pause_upload().await;
	}
	shared
		.store
		.lock()
		.uploads
		.insert(format!("{token}/{name}"), size);
	Ok(StatusCode::OK)
}

pub async fn inject_send_fault(
	State(shared): State<Arc<Shared>>,
	Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
	let fault = SendFault::parse(&name).ok_or_else(ApiError::not_found)?;
	*shared.send_fault.lock() = Some(fault);
	Ok(StatusCode::NO_CONTENT)
}

pub async fn upload_pace(
	State(shared): State<Arc<Shared>>,
	Path(pace): Path<String>,
) -> Result<StatusCode, ApiError> {
	let slow = match pace.as_str() {
		"slow" => true,
		"fast" => false,
		_ => return Err(ApiError::not_found()),
	};
	shared.slow_uploads.store(slow, Ordering::Relaxed);
	Ok(StatusCode::NO_CONTENT)
}

pub async fn account_premium(
	State(shared): State<Arc<Shared>>,
	Path(kind): Path<String>,
) -> Result<StatusCode, ApiError> {
	let kind = match kind.as_str() {
		"none" => PremiumType::None,
		"nitro" => PremiumType::Nitro,
		_ => return Err(ApiError::not_found()),
	};
	shared.set_premium(kind);
	Ok(StatusCode::NO_CONTENT)
}
