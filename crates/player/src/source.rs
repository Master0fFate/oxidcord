use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::Bytes;
use lru::LruCache;
use media::MediaFetcher;
use parking_lot::{Condvar, Mutex};
use symphonia::core::io::MediaSource;
use tokio::runtime::Handle;

const CHUNK: u64 = 512 * 1024;
const READ_AHEAD: u64 = 4;
const CACHED_CHUNKS: usize = 96;
const WAIT_SLICE: Duration = Duration::from_millis(100);
const ATTEMPTS: u32 = 3;

struct Store {
	chunks: LruCache<u64, Bytes>,
	pending: HashSet<u64>,
	failed: HashMap<u64, String>,
	total: Option<u64>,
}

struct Inner {
	store: Mutex<Store>,
	ready: Condvar,
	fetcher: MediaFetcher,
	runtime: Handle,
	url: String,
	closed: Arc<AtomicBool>,
}

pub struct HttpSource {
	inner: Arc<Inner>,
	position: u64,
	len: u64,
}

impl HttpSource {
	pub fn open(
		fetcher: MediaFetcher,
		runtime: Handle,
		url: String,
		closed: Arc<AtomicBool>,
	) -> io::Result<Self> {
		let capacity = NonZeroUsize::new(CACHED_CHUNKS).expect("cache capacity is not zero");
		let inner = Arc::new(Inner {
			store: Mutex::new(Store {
				chunks: LruCache::new(capacity),
				pending: HashSet::new(),
				failed: HashMap::new(),
				total: None,
			}),
			ready: Condvar::new(),
			fetcher,
			runtime,
			url,
			closed,
		});
		inner.chunk(0)?;
		let len = inner
			.store
			.lock()
			.total
			.ok_or_else(|| io::Error::other("the server did not report a length"))?;
		Ok(Self {
			inner,
			position: 0,
			len,
		})
	}
}

impl HttpSource {
	pub fn reopen(&self) -> Self {
		Self {
			inner: self.inner.clone(),
			position: 0,
			len: self.len,
		}
	}
}

impl Inner {
	fn request(self: &Arc<Self>, index: u64) {
		{
			let mut store = self.store.lock();
			if store.chunks.contains(&index) || !store.pending.insert(index) {
				return;
			}
			store.failed.remove(&index);
		}
		let inner = self.clone();
		self.runtime.spawn(async move {
			let start = index * CHUNK;
			let mut outcome = Err(media::MediaError::Fetch(String::new()));
			for _ in 0..ATTEMPTS {
				if inner.closed.load(Ordering::Relaxed) {
					break;
				}
				outcome = inner.fetcher.fetch_range(&inner.url, start, CHUNK).await;
				if outcome.is_ok() {
					break;
				}
			}
			let mut store = inner.store.lock();
			store.pending.remove(&index);
			match outcome {
				Ok(body) => {
					store.total = store.total.or(body.total);
					let mut offset = 0usize;
					while offset < body.bytes.len() {
						let end = (offset + CHUNK as usize).min(body.bytes.len());
						let at = (body.start + offset as u64) / CHUNK;
						store.chunks.put(at, body.bytes.slice(offset..end));
						offset = end;
					}
				}
				Err(error) => {
					store.failed.insert(index, error.to_string());
				}
			}
			drop(store);
			inner.ready.notify_all();
		});
	}

	fn chunk(self: &Arc<Self>, index: u64) -> io::Result<Bytes> {
		self.request(index);
		let mut store = self.store.lock();
		loop {
			if let Some(bytes) = store.chunks.get(&index) {
				return Ok(bytes.clone());
			}
			if let Some(error) = store.failed.remove(&index) {
				return Err(io::Error::other(error));
			}
			if self.closed.load(Ordering::Relaxed) {
				return Err(io::Error::new(io::ErrorKind::Interrupted, "playback closed"));
			}
			if !store.pending.contains(&index) {
				drop(store);
				self.request(index);
				store = self.store.lock();
				continue;
			}
			self.ready.wait_for(&mut store, WAIT_SLICE);
		}
	}
}

impl Read for HttpSource {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		if self.position >= self.len || buf.is_empty() {
			return Ok(0);
		}
		let index = self.position / CHUNK;
		let chunk = self.inner.chunk(index)?;
		for ahead in 1..=READ_AHEAD {
			if (index + ahead) * CHUNK < self.len {
				self.inner.request(index + ahead);
			}
		}
		let offset = (self.position % CHUNK) as usize;
		let available = chunk.len().saturating_sub(offset);
		if available == 0 {
			return Err(io::Error::new(
				io::ErrorKind::UnexpectedEof,
				"short range response",
			));
		}
		let count = available.min(buf.len());
		buf[..count].copy_from_slice(&chunk[offset..offset + count]);
		self.position += count as u64;
		Ok(count)
	}
}

impl Seek for HttpSource {
	fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
		let target = match position {
			SeekFrom::Start(offset) => Some(offset),
			SeekFrom::End(offset) => self.len.checked_add_signed(offset),
			SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
		};
		self.position =
			target.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
		Ok(self.position)
	}
}

impl MediaSource for HttpSource {
	fn is_seekable(&self) -> bool {
		true
	}

	fn byte_len(&self) -> Option<u64> {
		Some(self.len)
	}
}
