use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::stream;
use parking_lot::Mutex;
use reqwest::Body;

use crate::request::UploadProgress;

const REPORT_INTERVAL: Duration = Duration::from_millis(100);
const CHUNK_BYTES: usize = 64 * 1024;

struct Counter {
	sent: u64,
	reported: Instant,
}

pub(crate) struct Meter {
	report: UploadProgress,
	total: u64,
	counter: Mutex<Counter>,
}

impl Meter {
	pub(crate) fn new(report: UploadProgress, total: u64) -> Arc<Self> {
		Arc::new(Self {
			report,
			total,
			counter: Mutex::new(Counter {
				sent: 0,
				reported: Instant::now(),
			}),
		})
	}

	fn advance(&self, bytes: usize) {
		let sent = {
			let mut counter = self.counter.lock();
			counter.sent += bytes as u64;
			if counter.sent < self.total && counter.reported.elapsed() < REPORT_INTERVAL {
				return;
			}
			counter.reported = Instant::now();
			counter.sent
		};
		(self.report)(sent);
	}
}

pub(crate) fn metered_body(bytes: Bytes, meter: Arc<Meter>) -> Body {
	let chunks = stream::unfold((bytes, 0usize), move |(bytes, offset)| {
		let meter = Arc::clone(&meter);
		async move {
			if offset >= bytes.len() {
				return None;
			}
			let end = (offset + CHUNK_BYTES).min(bytes.len());
			let chunk = bytes.slice(offset..end);
			meter.advance(chunk.len());
			Some((Ok::<Bytes, Infallible>(chunk), (bytes, end)))
		}
	});
	Body::wrap_stream(chunks)
}
