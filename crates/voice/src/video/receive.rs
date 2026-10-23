use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

use tracing::warn;

use super::VideoStream;
use super::decode::H264Decoder;
use crate::shared::Shared;

const QUEUE_DEPTH: usize = 8;

pub(crate) struct DecodeJob {
	pub stream: VideoStream,
	pub data: Vec<u8>,
	pub timestamp: Duration,
}

pub(crate) struct DecodeWorker {
	jobs: SyncSender<DecodeJob>,
}

impl DecodeWorker {
	pub fn spawn(shared: Arc<Shared>) -> Option<Self> {
		let (jobs, receiver) = sync_channel(QUEUE_DEPTH);
		thread::Builder::new()
			.name("video-decode".to_owned())
			.spawn(move || run(&shared, &receiver))
			.ok()?;
		Some(Self { jobs })
	}

	pub fn submit(&self, job: DecodeJob) -> bool {
		!matches!(
			self.jobs.try_send(job),
			Err(TrySendError::Full(_) | TrySendError::Disconnected(_))
		)
	}
}

fn run(shared: &Shared, receiver: &Receiver<DecodeJob>) {
	let mut decoders: HashMap<u32, H264Decoder> = HashMap::new();
	while let Ok(job) = receiver.recv() {
		let Some(sink) = shared.video.sink.lock().clone() else {
			continue;
		};
		let ssrc = job.stream.ssrc;
		let decoder = match decoders.entry(ssrc) {
			Entry::Occupied(entry) => entry.into_mut(),
			Entry::Vacant(entry) => match H264Decoder::new() {
				Ok(decoder) => entry.insert(decoder),
				Err(error) => {
					warn!(%error, "failed to create an h264 decoder");
					continue;
				}
			},
		};
		match decoder.decode(&job.data, job.timestamp) {
			Ok(Some(frame)) => sink.frame(job.stream, Arc::new(frame)),
			Ok(None) => {}
			Err(error) => {
				warn!(%error, ssrc, "h264 decode failed");
				decoders.remove(&ssrc);
				shared.counters.video_dropped.fetch_add(1, Ordering::Relaxed);
				shared.video.report_decode_failure(ssrc);
			}
		}
	}
}
