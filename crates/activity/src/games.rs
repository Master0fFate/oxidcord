use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::Utc;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, sleep_until};
use url::Url;

use crate::engine::Input;
use crate::processes::{self, Process};

const CACHE_FILE: &str = "detectable.json";
const CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_CATALOG_BYTES: usize = 64 * 1024 * 1024;
const SCAN_INTERVAL: Duration = Duration::from_secs(5);
const FETCH_RETRY: Duration = Duration::from_secs(10 * 60);

#[cfg(windows)]
const CURRENT_OS: &str = "win32";
#[cfg(target_os = "macos")]
const CURRENT_OS: &str = "darwin";
#[cfg(not(any(windows, target_os = "macos")))]
const CURRENT_OS: &str = "linux";

pub(crate) struct GamesConfig {
	pub rest_base: Url,
	pub http: Option<reqwest::Client>,
	pub cache_dir: PathBuf,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Executable {
	name: String,
	os: String,
	is_launcher: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DetectableApp {
	id: String,
	name: String,
	executables: Vec<Executable>,
}

struct Entry {
	app_id: u64,
	name: Arc<str>,
	suffix: String,
}

#[derive(Default)]
struct Catalog {
	by_file: HashMap<String, Vec<Entry>>,
}

impl Catalog {
	fn parse(bytes: &[u8]) -> Option<Self> {
		let apps: Vec<DetectableApp> = serde_json::from_slice(bytes).ok()?;
		let mut catalog = Self::default();
		for app in apps {
			let Ok(app_id) = app.id.parse::<u64>() else {
				continue;
			};
			let name: Arc<str> = Arc::from(app.name.as_str());
			for executable in app.executables {
				if executable.is_launcher || executable.os != CURRENT_OS {
					continue;
				}
				let suffix = executable.name.replace('\\', "/").to_lowercase();
				let Some(file) = suffix.rsplit('/').next().filter(|file| !file.is_empty()) else {
					continue;
				};
				catalog.by_file.entry(file.to_owned()).or_default().push(Entry {
					app_id,
					name: name.clone(),
					suffix,
				});
			}
		}
		Some(catalog)
	}

	fn lookup(&self, process: &Process) -> Option<&Entry> {
		let file = process.path.rsplit('/').next()?;
		self.by_file.get(file)?.iter().find(|entry| {
			process.path.ends_with(&entry.suffix)
				&& (process.path.len() == entry.suffix.len()
					|| process.path.as_bytes()[process.path.len() - entry.suffix.len() - 1] == b'/')
		})
	}
}

struct Detected {
	name: Arc<str>,
	since: i64,
}

fn cache_age(path: &Path) -> Option<Duration> {
	let modified = std::fs::metadata(path).ok()?.modified().ok()?;
	SystemTime::now().duration_since(modified).ok()
}

async fn fetch(http: &reqwest::Client, rest_base: &Url, cache: &Path) -> Option<Vec<u8>> {
	let url = format!(
		"{}/applications/detectable",
		rest_base.as_str().trim_end_matches('/')
	);
	let response = http.get(&url).send().await.ok()?.error_for_status().ok()?;
	let mut body = Vec::new();
	let mut stream = response.bytes_stream();
	while let Some(chunk) = stream.next().await {
		let chunk = chunk.ok()?;
		if body.len() + chunk.len() > MAX_CATALOG_BYTES {
			return None;
		}
		body.extend_from_slice(&chunk);
	}
	if let Some(parent) = cache.parent() {
		let _ = std::fs::create_dir_all(parent);
	}
	let temporary = cache.with_extension("json.part");
	if std::fs::write(&temporary, &body).is_ok() && std::fs::rename(&temporary, cache).is_err() {
		let _ = std::fs::remove_file(&temporary);
	}
	Some(body)
}

async fn load(config: &GamesConfig) -> (Option<Arc<Catalog>>, Instant) {
	let cache = config.cache_dir.join(CACHE_FILE);
	let age = cache_age(&cache);
	let fresh = age.is_some_and(|age| age < CACHE_MAX_AGE);
	let mut bytes = if fresh { std::fs::read(&cache).ok() } else { None };
	let mut next_check = Instant::now() + CACHE_MAX_AGE.saturating_sub(age.unwrap_or_default());
	if bytes.is_none() {
		bytes = match &config.http {
			Some(http) => fetch(http, &config.rest_base, &cache).await,
			None => None,
		};
		next_check = Instant::now() + CACHE_MAX_AGE;
		if bytes.is_none() {
			bytes = std::fs::read(&cache).ok();
			next_check = Instant::now() + FETCH_RETRY;
		}
	}
	let catalog = bytes.and_then(|bytes| Catalog::parse(&bytes)).map(Arc::new);
	match &catalog {
		Some(catalog) => tracing::debug!(executables = catalog.by_file.len(), "game catalog loaded"),
		None => tracing::debug!("game catalog is unavailable"),
	}
	(catalog, next_check)
}

fn activity(app_id: u64, detected: &Detected) -> Value {
	json!({
		"type": 0,
		"name": detected.name.as_ref(),
		"application_id": app_id.to_string(),
		"timestamps": { "start": detected.since },
		"created_at": detected.since,
	})
}

async fn wait_enabled(enabled: &mut watch::Receiver<bool>) -> bool {
	loop {
		if *enabled.borrow() {
			return true;
		}
		if enabled.changed().await.is_err() {
			return false;
		}
	}
}

pub(crate) async fn run(
	config: GamesConfig,
	mut enabled: watch::Receiver<bool>,
	inputs: mpsc::UnboundedSender<Input>,
) {
	let mut catalog: Option<Arc<Catalog>> = None;
	let mut next_check = Instant::now();
	let mut running: BTreeMap<u64, Detected> = BTreeMap::new();
	let mut published: Option<u64> = None;
	loop {
		if !*enabled.borrow() {
			if published.take().is_some() {
				let _ = inputs.send(Input::Game(None));
			}
			running.clear();
			if !wait_enabled(&mut enabled).await {
				return;
			}
		}
		if Instant::now() >= next_check {
			let (loaded, check) = load(&config).await;
			if loaded.is_some() {
				catalog = loaded;
			}
			next_check = check;
		}
		if let Some(catalog) = catalog.clone() {
			let scan = tokio::task::spawn_blocking(move || {
				let mut found: BTreeMap<u64, Arc<str>> = BTreeMap::new();
				for process in processes::running() {
					if let Some(entry) = catalog.lookup(&process) {
						found.entry(entry.app_id).or_insert_with(|| entry.name.clone());
					}
				}
				found
			})
			.await
			.unwrap_or_default();
			let now = Utc::now().timestamp_millis();
			running.retain(|app_id, _| scan.contains_key(app_id));
			for (app_id, name) in scan {
				running
					.entry(app_id)
					.or_insert_with(|| Detected { name, since: now });
			}
			let current = running
				.iter()
				.min_by_key(|(_, detected)| detected.since)
				.map(|(app_id, _)| *app_id);
			if current != published {
				published = current;
				let game = current
					.and_then(|app_id| running.get(&app_id).map(|detected| activity(app_id, detected)));
				let _ = inputs.send(Input::Game(game));
			}
		}
		tokio::select! {
			() = sleep_until(Instant::now() + SCAN_INTERVAL) => {}
			changed = enabled.changed() => {
				if changed.is_err() {
					return;
				}
			}
		}
	}
}
