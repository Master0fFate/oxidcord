use std::collections::{HashMap, HashSet, VecDeque};
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
use crate::lottie;
use crate::resize::resize_decoded;
use crate::twemoji;
use crate::types::Decoded;

const FAILURE_RETRY_DELAY: Duration = Duration::from_secs(60);
const MAX_IN_FLIGHT: usize = 6;

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
	in_flight: Mutex<usize>,
	queued: Mutex<VecDeque<(u64, String, SizeHint, bool)>>,
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
				in_flight: Mutex::new(0),
				queued: Mutex::new(VecDeque::new()),
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

	pub fn fetcher(&self) -> &MediaFetcher {
		&self.inner.fetcher
	}

	pub fn runtime(&self) -> &Handle {
		&self.inner.runtime
	}

	pub fn peek(&self, url: &str, hint: SizeHint, still: bool) -> Option<Arc<Decoded>> {
		self.inner.memory.lock().get(url, hint, still)
	}

	pub fn request(&self, url: &str, hint: SizeHint, still: bool) {
		if self.peek(url, hint, still).is_some() {
			return;
		}
		let key = memory_key(url, hint, still);
		if self.recently_failed(key) {
			return;
		}
		let inner = &self.inner;
		if !inner.pending.lock().insert(key) {
			return;
		}
		let mut in_flight = inner.in_flight.lock();
		if *in_flight < MAX_IN_FLIGHT {
			*in_flight += 1;
			drop(in_flight);
			self.start_load(key, url.to_owned(), hint, still);
		} else if still {
			inner.queued.lock().push_back((key, url.to_owned(), hint, still));
		} else {
			inner.queued.lock().push_front((key, url.to_owned(), hint, still));
		}
	}

	fn start_load(&self, key: u64, url: String, hint: SizeHint, still: bool) {
		let service = self.clone();
		self.inner.runtime.spawn(async move {
			let succeeded = match service.load(&url, hint, still).await {
				Ok(_) => true,
				Err(error) => {
					tracing::debug!(url = redact_url(&url), %error, "media load failed");
					false
				}
			};
			if succeeded {
				service.notify_ready();
			}
			service.finish(key, succeeded);
		});
	}

	fn finish(&self, key: u64, succeeded: bool) {
		let inner = &self.inner;
		inner.pending.lock().remove(&key);
		if !succeeded {
			inner.failed.lock().insert(key, Instant::now());
		}
		let mut in_flight = inner.in_flight.lock();
		*in_flight -= 1;
		let next = if *in_flight < MAX_IN_FLIGHT {
			inner.queued.lock().pop_front()
		} else {
			None
		};
		if let Some((next_key, next_url, next_hint, next_still)) = next {
			*in_flight += 1;
			self.start_load(next_key, next_url, next_hint, next_still);
		}
	}

	pub async fn load(&self, url: &str, hint: SizeHint, still: bool) -> Result<Arc<Decoded>> {
		if let Some(hit) = self.peek(url, hint, still) {
			return Ok(hit);
		}
		let (bytes, from_disk) = self.raw_bytes(url).await?;
		let mut limits = self.inner.limits.clone();
		limits.first_frame_only = still;
		let source = bytes.clone();
		let decoded = spawn_blocking(move || {
			if lottie::is_lottie(&source) {
				lottie::decode(&source, &limits, hint)
			} else {
				decode(&source, &limits).map(|decoded| shrink_to_hint(decoded, hint))
			}
		})
		.await
		.map_err(|error| MediaError::Decode(format!("decode task failed: {error}")))??;
		if !from_disk {
			self.store_on_disk(url, bytes).await;
		}
		let decoded = Arc::new(decoded);
		self.inner.memory.lock().insert(url, hint, still, decoded.clone());
		Ok(decoded)
	}

	async fn raw_bytes(&self, url: &str) -> Result<(Bytes, bool)> {
		if let Some(bundled) = twemoji::bytes(url) {
			return Ok((bundled, true));
		}
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
	if width <= hint.width && height <= hint.height {
		return decoded;
	}
	resize_decoded(decoded, hint.width, hint.height)
}
