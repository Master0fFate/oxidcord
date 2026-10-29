use axum::Json;
use axum::extract::Path;
use serde_json::{Value, json};

use super::apps::NOTES_APP_ID;
use crate::error::{ApiError, ApiResult};

pub const RPC_APP_ID: u64 = NOTES_APP_ID;
pub const RPC_APP_NAME: &str = "Pocket Notes";
pub const RPC_ASSET_NAME: &str = "logo";
pub const RPC_ASSET_ID: u64 = NOTES_APP_ID + 10;
const RPC_ORIGIN: &str = "http://localhost:1234";

fn known(id: u64) -> Result<(), ApiError> {
	if id == RPC_APP_ID {
		Ok(())
	} else {
		Err(ApiError::not_found())
	}
}

pub async fn rpc_application(Path(id): Path<u64>) -> ApiResult {
	known(id)?;
	Ok(Json(json!({
		"id": id.to_string(),
		"name": RPC_APP_NAME,
		"icon": null,
		"description": "",
		"rpc_origins": [RPC_ORIGIN],
		"flags": 0,
		"hook": true,
	})))
}

pub async fn application_assets(Path(id): Path<u64>) -> ApiResult {
	known(id)?;
	Ok(Json(json!([
		{ "id": RPC_ASSET_ID.to_string(), "type": 1, "name": RPC_ASSET_NAME },
	])))
}

pub async fn external_assets(Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	known(id)?;
	let proxied: Vec<Value> = body["urls"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(|url| {
			let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
			json!({
				"url": url,
				"external_asset_path": format!("external/mock/{scheme}/{rest}"),
			})
		})
		.collect();
	Ok(Json(Value::Array(proxied)))
}

pub async fn connections() -> Json<Value> {
	Json(json!([]))
}

pub async fn detectable() -> Json<Value> {
	Json(json!([]))
}
