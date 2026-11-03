use serde::Deserialize;
use serde_json::json;

use crate::client::RestClient;
use crate::error::{RestError, Result};
use crate::request::{Request, encode_component};

#[derive(Deserialize, Default)]
#[serde(default)]
struct PreviewPayload {
	url: Option<String>,
}

fn preview_path(stream_key: &str) -> String {
	format!("/streams/{}/preview", encode_component(stream_key))
}

impl RestClient {
	pub async fn stream_preview(&self, stream_key: &str) -> Result<Option<String>> {
		match self
			.send::<PreviewPayload>(Request::get(preview_path(stream_key)))
			.await
		{
			Ok(payload) => Ok(payload.url),
			Err(RestError::NotFound(_)) => Ok(None),
			Err(error) => Err(error),
		}
	}

	pub async fn upload_stream_preview(&self, stream_key: &str, thumbnail: &str) -> Result<()> {
		let request = Request::post(preview_path(stream_key)).json(&json!({ "thumbnail": thumbnail }));
		self.send_empty(request).await
	}
}
