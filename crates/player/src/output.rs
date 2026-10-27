use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use parking_lot::Mutex;

use crate::error::{PlayerError, Result};

pub const OUTPUT_CHANNELS: usize = 2;
const SILENT_RATE: u32 = 48_000;
const SILENT_TICK: Duration = Duration::from_millis(10);

struct Ring {
	samples: Mutex<VecDeque<f32>>,
	played: AtomicU64,
	last_chunk: AtomicU64,
	stamp: AtomicU64,
	epoch: Instant,
	volume: Arc<AtomicU32>,
}

enum Sink {
	Device(Stream),
	Silent(SilentClock),
}

struct SilentClock {
	playing: Arc<AtomicBool>,
	alive: Arc<AtomicBool>,
}

impl SilentClock {
	fn start(ring: Arc<Ring>) -> Self {
		let playing = Arc::new(AtomicBool::new(false));
		let alive = Arc::new(AtomicBool::new(true));
		let (thread_playing, thread_alive) = (playing.clone(), alive.clone());
		let spawned = thread::Builder::new().name("silent-audio".into()).spawn(move || {
			let mut buffer = Vec::new();
			let mut last = Instant::now();
			while thread_alive.load(Ordering::Acquire) {
				thread::sleep(SILENT_TICK);
				let now = Instant::now();
				let frames = (now.duration_since(last).as_secs_f64() * f64::from(SILENT_RATE)) as usize;
				last = now;
				if frames > 0 && thread_playing.load(Ordering::Acquire) {
					buffer.clear();
					buffer.resize(frames * OUTPUT_CHANNELS, 0.0f32);
					fill(&mut buffer, OUTPUT_CHANNELS, &ring);
				}
			}
		});
		if let Err(error) = spawned {
			tracing::warn!(%error, "could not start the silent audio clock");
		}
		Self { playing, alive }
	}
}

impl Drop for SilentClock {
	fn drop(&mut self) {
		self.alive.store(false, Ordering::Release);
	}
}

pub struct Output {
	sink: Sink,
	ring: Arc<Ring>,
	rate: u32,
}

impl Output {
	pub fn open(volume: Arc<AtomicU32>) -> Result<Self> {
		let ring = Arc::new(Ring {
			samples: Mutex::new(VecDeque::new()),
			played: AtomicU64::new(0),
			last_chunk: AtomicU64::new(0),
			stamp: AtomicU64::new(0),
			epoch: Instant::now(),
			volume,
		});
		let (sink, rate) = match device_stream(&ring) {
			Ok((stream, rate)) => (Sink::Device(stream), rate),
			Err(error) => {
				tracing::info!(%error, "no audio output, media plays silently");
				(Sink::Silent(SilentClock::start(ring.clone())), SILENT_RATE)
			}
		};
		Ok(Self { sink, ring, rate })
	}

	pub fn rate(&self) -> u32 {
		self.rate
	}

	pub fn push(&self, stereo: &[f32]) {
		self.ring.samples.lock().extend(stereo);
	}

	pub fn buffered_frames(&self) -> usize {
		self.ring.samples.lock().len() / OUTPUT_CHANNELS
	}

	pub fn played(&self) -> Duration {
		let rate = f64::from(self.rate);
		let frames = self.ring.played.load(Ordering::Acquire) as f64;
		let chunk = self.ring.last_chunk.load(Ordering::Acquire) as f64 / rate;
		let since = self
			.ring
			.epoch
			.elapsed()
			.saturating_sub(Duration::from_nanos(self.ring.stamp.load(Ordering::Acquire)))
			.as_secs_f64();
		Duration::from_secs_f64((frames / rate - chunk).max(0.) + since.min(chunk))
	}

	pub fn reset(&self) {
		self.ring.samples.lock().clear();
		self.ring.last_chunk.store(0, Ordering::Release);
		self.ring.played.store(0, Ordering::Release);
	}

	pub fn resume(&self) {
		match &self.sink {
			Sink::Device(stream) => {
				if let Err(error) = stream.play() {
					tracing::debug!(%error, "could not resume media audio");
				}
			}
			Sink::Silent(clock) => clock.playing.store(true, Ordering::Release),
		}
	}

	pub fn pause(&self) {
		self.ring.last_chunk.store(0, Ordering::Release);
		match &self.sink {
			Sink::Device(stream) => {
				if let Err(error) = stream.pause() {
					tracing::debug!(%error, "could not pause media audio");
				}
			}
			Sink::Silent(clock) => clock.playing.store(false, Ordering::Release),
		}
	}
}

fn device_stream(ring: &Arc<Ring>) -> Result<(Stream, u32)> {
	let device = cpal::default_host()
		.default_output_device()
		.ok_or_else(|| PlayerError::Audio("no output device".to_owned()))?;
	let supported = device
		.default_output_config()
		.map_err(|error| PlayerError::Audio(error.to_string()))?;
	let config = supported.config();
	let stream = match supported.sample_format() {
		SampleFormat::F32 => build::<f32>(&device, &config, ring),
		SampleFormat::I16 => build::<i16>(&device, &config, ring),
		SampleFormat::U16 => build::<u16>(&device, &config, ring),
		SampleFormat::I32 => build::<i32>(&device, &config, ring),
		other => return Err(PlayerError::Audio(format!("unsupported sample format {other}"))),
	}?;
	Ok((stream, config.sample_rate))
}

fn build<T>(device: &cpal::Device, config: &StreamConfig, ring: &Arc<Ring>) -> Result<Stream>
where
	T: SizedSample + FromSample<f32>,
{
	let channels = usize::from(config.channels);
	let ring = ring.clone();
	let stream = device
		.build_output_stream(
			*config,
			move |data: &mut [T], _| fill(data, channels, &ring),
			|error| tracing::debug!(%error, "media audio stream error"),
			None,
		)
		.map_err(|error| PlayerError::Audio(error.to_string()))?;
	stream
		.pause()
		.map_err(|error| PlayerError::Audio(error.to_string()))?;
	Ok(stream)
}

fn fill<T: SizedSample + FromSample<f32>>(data: &mut [T], channels: usize, ring: &Ring) {
	let volume = f32::from_bits(ring.volume.load(Ordering::Relaxed));
	let mut samples = ring.samples.lock();
	let mut played = 0u64;
	for frame in data.chunks_exact_mut(channels) {
		let (left, right) = match (samples.pop_front(), samples.pop_front()) {
			(Some(left), Some(right)) => {
				played += 1;
				(left * volume, right * volume)
			}
			_ => (0.0, 0.0),
		};
		if channels == 1 {
			frame[0] = T::from_sample((left + right) * 0.5);
			continue;
		}
		frame[0] = T::from_sample(left);
		frame[1] = T::from_sample(right);
		for extra in &mut frame[2..] {
			*extra = T::from_sample(0.0);
		}
	}
	drop(samples);
	ring.played.fetch_add(played, Ordering::AcqRel);
	ring.last_chunk.store(played, Ordering::Release);
	ring.stamp
		.store(ring.epoch.elapsed().as_nanos() as u64, Ordering::Release);
}
