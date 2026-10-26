use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use bytes::Bytes;
use lru::LruCache;
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::types::Decoded;

pub const DEFAULT_MEMORY_LIMIT_BYTES: usize = 256 * 1024 * 1024;

const SIGNATURE_PARAMS: [&str; 3] = ["ex", "is", "hm"];
const SHARD_LEN: usize = 2;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SizeHint {
	pub width: u32,
	pub height: u32,
}

impl SizeHint {
	pub const ORIGINAL: Self = Self { width: 0, height: 0 };

	pub const fn new(width: u32, height: u32) -> Self {
		Self { width, height }
	}

	pub const fn square(side: u32) -> Self {
		Self {
			width: side,
			height: side,
		}
	}

	pub fn is_original(self) -> bool {
		self.width == 0 || self.height == 0
	}
}

struct MemoryEntry {
	url: Box<str>,
	hint: SizeHint,
	still: bool,
	value: Arc<Decoded>,
	bytes: usize,
}

pub struct MemoryCache {
	entries: LruCache<u64, MemoryEntry>,
	bytes: usize,
	limit: usize,
}

impl MemoryCache {
	pub fn new(limit_bytes: usize) -> Self {
		Self {
			entries: LruCache::unbounded(),
			bytes: 0,
			limit: limit_bytes,
		}
	}

	pub fn get(&mut self, url: &str, hint: SizeHint, still: bool) -> Option<Arc<Decoded>> {
		let entry = self.entries.get(&memory_key(url, hint, still))?;
		(&*entry.url == url && entry.hint == hint && entry.still == still).then(|| entry.value.clone())
	}

	pub fn insert(&mut self, url: &str, hint: SizeHint, still: bool, value: Arc<Decoded>) {
		let bytes = value.byte_len();
		if bytes > self.limit {
			return;
		}
		let entry = MemoryEntry {
			url: url.into(),
			hint,
			still,
			value,
			bytes,
		};
		if let Some(replaced) = self.entries.put(memory_key(url, hint, still), entry) {
			self.bytes -= replaced.bytes;
		}
		self.bytes += bytes;
		self.evict();
	}

	pub fn set_limit(&mut self, limit_bytes: usize) {
		self.limit = limit_bytes;
		self.evict();
	}

	pub fn len(&self) -> usize {
		self.entries.len()
	}

	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	pub fn bytes(&self) -> usize {
		self.bytes
	}

	fn evict(&mut self) {
		while self.bytes > self.limit {
			let Some((_, evicted)) = self.entries.pop_lru() else {
				break;
			};
			self.bytes -= evicted.bytes;
		}
	}
}

pub(crate) fn memory_key(url: &str, hint: SizeHint, still: bool) -> u64 {
	let mut hasher = DefaultHasher::new();
	url.hash(&mut hasher);
	hint.hash(&mut hasher);
	still.hash(&mut hasher);
	hasher.finish()
}

pub struct DiskCache {
	root: PathBuf,
}

impl DiskCache {
	pub fn new(cache_dir: impl AsRef<Path>) -> Self {
		Self {
			root: cache_dir.as_ref().join("media"),
		}
	}

	pub fn root(&self) -> &Path {
		&self.root
	}

	pub fn put(&self, url: &str, bytes: &[u8]) -> Result<()> {
		let path = self.path_for(url);
		let shard = path.parent().expect("cache path always has a shard directory");
		fs::create_dir_all(shard)?;
		let temp = shard.join(format!(
			".{}-{}.tmp",
			std::process::id(),
			TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
		));
		let written = fs::File::create(&temp).and_then(|mut file| file.write_all(bytes));
		if let Err(error) = written.and_then(|()| fs::rename(&temp, &path)) {
			let _ = fs::remove_file(&temp);
			return Err(error.into());
		}
		Ok(())
	}

	pub fn get(&self, url: &str) -> Result<Option<Bytes>> {
		let mut file = match OpenOptions::new().read(true).write(true).open(self.path_for(url)) {
			Ok(file) => file,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
			Err(error) => return Err(error.into()),
		};
		let mut contents = Vec::with_capacity(file.metadata()?.len() as usize);
		file.read_to_end(&mut contents)?;
		let _ = file.set_modified(SystemTime::now());
		Ok(Some(contents.into()))
	}

	pub fn evict_to(&self, limit_bytes: u64) -> Result<u64> {
		let shards = match fs::read_dir(&self.root) {
			Ok(shards) => shards,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
			Err(error) => return Err(error.into()),
		};
		let mut files = Vec::new();
		let mut total = 0u64;
		for shard in shards {
			let shard = shard?;
			if !shard.file_type()?.is_dir() {
				continue;
			}
			for entry in fs::read_dir(shard.path())? {
				let entry = entry?;
				let metadata = entry.metadata()?;
				if metadata.is_file() {
					total += metadata.len();
					files.push((metadata.modified()?, metadata.len(), entry.path()));
				}
			}
		}
		files.sort_unstable_by_key(|(modified, _, _)| *modified);
		let mut removed = 0u64;
		for (_, length, path) in files {
			if total <= limit_bytes {
				break;
			}
			if fs::remove_file(path).is_ok() {
				total -= length;
				removed += length;
			}
		}
		Ok(removed)
	}

	fn path_for(&self, url: &str) -> PathBuf {
		let digest = hex::encode(Sha256::digest(canonical_url(url).as_bytes()));
		self.root.join(&digest[..SHARD_LEN]).join(digest)
	}
}

fn canonical_url(url: &str) -> Cow<'_, str> {
	let Some((base, rest)) = url.split_once('?') else {
		return Cow::Borrowed(url);
	};
	let query = rest.split('#').next().unwrap_or_default();
	let kept: Vec<&str> = query
		.split('&')
		.filter(|pair| {
			let name = pair.split('=').next().unwrap_or_default();
			!SIGNATURE_PARAMS.contains(&name)
		})
		.collect();
	if kept.is_empty() {
		Cow::Borrowed(base)
	} else {
		Cow::Owned(format!("{base}?{}", kept.join("&")))
	}
}
