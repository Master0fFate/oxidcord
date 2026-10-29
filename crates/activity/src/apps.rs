use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use protocol::Snowflake;
use rest::RestClient;
use serde_json::Value;

const NEGATIVE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub(crate) struct RpcApp {
	pub id: Snowflake,
	pub name: String,
	pub rpc_origins: Vec<String>,
}

impl RpcApp {
	fn parse(raw: &Value) -> Option<Self> {
		let id = raw.get("id")?.as_str()?.parse().ok()?;
		let name = raw.get("name")?.as_str()?.to_owned();
		let rpc_origins = raw
			.get("rpc_origins")
			.and_then(Value::as_array)
			.map(|origins| {
				origins
					.iter()
					.filter_map(Value::as_str)
					.map(|origin| origin.trim_end_matches('/').to_ascii_lowercase())
					.collect()
			})
			.unwrap_or_default();
		Some(Self {
			id: Snowflake(id),
			name,
			rpc_origins,
		})
	}

	pub fn allows_origin(&self, origin: &str) -> bool {
		let origin = origin.trim_end_matches('/').to_ascii_lowercase();
		self.rpc_origins.contains(&origin)
	}
}

struct Lookup {
	at: Instant,
	app: Option<Arc<RpcApp>>,
}

pub(crate) struct Apps {
	rest: RestClient,
	apps: Mutex<HashMap<u64, Lookup>>,
	assets: Mutex<HashMap<u64, Arc<HashMap<String, String>>>>,
	external: Mutex<HashMap<(u64, String), String>>,
}

impl Apps {
	pub fn new(rest: RestClient) -> Self {
		Self {
			rest,
			apps: Mutex::new(HashMap::new()),
			assets: Mutex::new(HashMap::new()),
			external: Mutex::new(HashMap::new()),
		}
	}

	pub async fn rpc_app(&self, id: u64) -> Option<Arc<RpcApp>> {
		if let Some(lookup) = self.apps.lock().get(&id)
			&& (lookup.app.is_some() || lookup.at.elapsed() < NEGATIVE_TTL)
		{
			return lookup.app.clone();
		}
		let app = match self.rest.application_rpc(Snowflake(id)).await {
			Ok(raw) => RpcApp::parse(&raw).map(Arc::new),
			Err(error) => {
				tracing::debug!(%error, id, "rpc application lookup failed");
				None
			}
		};
		self.apps.lock().insert(
			id,
			Lookup {
				at: Instant::now(),
				app: app.clone(),
			},
		);
		app
	}

	pub async fn asset_id(&self, app: u64, name: &str) -> Option<String> {
		let known = self.assets.lock().get(&app).cloned();
		let assets = match known {
			Some(assets) => assets,
			None => {
				let raw = self.rest.application_assets(Snowflake(app)).await.ok()?;
				let assets: HashMap<String, String> = raw
					.as_array()
					.into_iter()
					.flatten()
					.filter_map(|asset| {
						Some((
							asset.get("name")?.as_str()?.to_ascii_lowercase(),
							asset.get("id")?.as_str()?.to_owned(),
						))
					})
					.collect();
				let assets = Arc::new(assets);
				self.assets.lock().insert(app, assets.clone());
				assets
			}
		};
		assets.get(&name.to_ascii_lowercase()).cloned()
	}

	pub async fn external(&self, app: u64, url: &str) -> Option<String> {
		let key = (app, url.to_owned());
		if let Some(path) = self.external.lock().get(&key) {
			return Some(path.clone());
		}
		let raw = self
			.rest
			.external_assets(Snowflake(app), std::slice::from_ref(&key.1))
			.await
			.ok()?;
		let path = raw
			.as_array()?
			.iter()
			.find(|entry| entry.get("url").and_then(Value::as_str) == Some(url))
			.and_then(|entry| entry.get("external_asset_path")?.as_str())
			.map(|path| format!("mp:{path}"))?;
		self.external.lock().insert(key, path.clone());
		Some(path)
	}
}
