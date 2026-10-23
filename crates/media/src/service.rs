use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use tokio::runtime::Handle;
use tokio::task::spawn_blocking;

use crate::cache::{DEFAULT_MEMORY_LIMIT_BYTES, DiskCache, MemoryCache, SizeHint, memory_key};
use crate::decode::{Limits, decode};
use crate::error::{MediaError, Result, redact_url};
use crate::fetch::MediaFetcher;
use crate::resize::resize_decoded;
use crate::types::Decoded;

const SHRINK_THRESHOLD: u32 = 2;
const FAILURE_RETRY_DELAY: Duration = Duration::from_secs(60);

type ReadyCallback = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct MediaService {
	inner: Arc<ServiceInner>,
}

struct ServiceInner {
	fetcher: MediaFetcher,
	disk: Arc<DiskCache>,
	memory: Mutex<MemoryCache>,
	limits: Limits,
	pending: Mutex<HashSet<u64>>,
	failed: Mutex<HashMap<u64, Instant>>,
	on_ready: RwLock<Option<ReadyCallback>>,
	runtime: Handle,
}

impl MediaService {
	pub fn new(fetcher: MediaFetcher, disk_dir: impl Into<PathBuf>, limits: Limits) -> Self {
		Self {
			inner: Arc::new(ServiceInner {
				fetcher,
				disk: Arc::new(DiskCache::new(disk_dir.into())),
				memory: Mutex::new(MemoryCache::new(DEFAULT_MEMORY_LIMIT_BYTES)),
				limits,
				pending: Mutex::new(HashSet::new()),
				failed: Mutex::new(HashMap::new()),
				on_ready: RwLock::new(None),
				runtime: Handle::current(),
			}),
		}
	}

	pub fn set_memory_limit(&self, limit_bytes: usize) {
		self.inner.memory.lock().set_limit(limit_bytes);
	}

	pub fn set_on_ready(&self, callback: impl Fn() + Send + Sync + 'static) {
		*self.inner.on_ready.write() = Some(Arc::new(callback));
	}

	pub fn disk(&self) -> &DiskCache {
		&self.inner.disk
	}

	pub fn peek(&self, url: &str, hint: SizeHint) -> Option<Arc<Decoded>> {
		self.inner.memory.lock().get(url, hint)
	}

	pub fn request(&self, url: &str, hint: SizeHint) {
		if self.peek(url, hint).is_some() {
			return;
		}
		let key = memory_key(url, hint);
		if self.recently_failed(key) || !self.inner.pending.lock().insert(key) {
			return;
		}
		let service = self.clone();
		let url = url.to_owned();
		self.inner.runtime.spawn(async move {
			match service.load(&url, hint).await {
				Ok(_) => service.notify_ready(),
				Err(error) => {
					tracing::debug!(url = redact_url(&url), %error, "media load failed");
					service.inner.failed.lock().insert(key, Instant::now());
				}
			}
			service.inner.pending.lock().remove(&key);
		});
	}

	pub async fn load(&self, url: &str, hint: SizeHint) -> Result<Arc<Decoded>> {
		if let Some(hit) = self.peek(url, hint) {
			return Ok(hit);
		}
		let (bytes, from_disk) = self.raw_bytes(url).await?;
		let limits = self.inner.limits.clone();
		let source = bytes.clone();
		let decoded =
			spawn_blocking(move || decode(&source, &limits).map(|decoded| shrink_to_hint(decoded, hint)))
				.await
				.map_err(|error| MediaError::Decode(format!("decode task failed: {error}")))??;
		if !from_disk {
			self.store_on_disk(url, bytes).await;
		}
		let decoded = Arc::new(decoded);
		self.inner.memory.lock().insert(url, hint, decoded.clone());
		Ok(decoded)
	}

	async fn raw_bytes(&self, url: &str) -> Result<(Bytes, bool)> {
		let disk = self.inner.disk.clone();
		let owned = url.to_owned();
		let cached = spawn_blocking(move || disk.get(&owned))
			.await
			.ok()
			.and_then(|result| result.ok())
			.flatten();
		match cached {
			Some(bytes) => Ok((bytes, true)),
			None => Ok((self.inner.fetcher.fetch(url).await?, false)),
		}
	}

	async fn store_on_disk(&self, url: &str, bytes: Bytes) {
		let disk = self.inner.disk.clone();
		let owned = url.to_owned();
		let stored = spawn_blocking(move || disk.put(&owned, &bytes)).await;
		if let Ok(Err(error)) = stored {
			tracing::debug!(url = redact_url(url), %error, "media disk write failed");
		}
	}

	fn recently_failed(&self, key: u64) -> bool {
		let mut failed = self.inner.failed.lock();
		match failed.get(&key) {
			Some(at) if at.elapsed() < FAILURE_RETRY_DELAY => true,
			Some(_) => {
				failed.remove(&key);
				false
			}
			None => false,
		}
	}

	fn notify_ready(&self) {
		let callback = self.inner.on_ready.read().clone();
		if let Some(callback) = callback {
			callback();
		}
	}
}

fn shrink_to_hint(decoded: Decoded, hint: SizeHint) -> Decoded {
	if hint.is_original() {
		return decoded;
	}
	let (width, height) = decoded.dimensions();
	if width > hint.width * SHRINK_THRESHOLD || height > hint.height * SHRINK_THRESHOLD {
		resize_decoded(decoded, hint.width, hint.height)
	} else {
		decoded
	}
}
