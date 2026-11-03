use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::audio::{AudioEngine, AudioRoute, Rings};
use crate::chain::{CaptureChain, PlaybackChain};
use crate::codec::{FRAME_SAMPLES, Profile, STEREO_FRAME_SAMPLES, VoiceDecoder, VoiceEncoder};
use crate::denoise::Denoiser;
use crate::error::{Result, VoiceError};
use crate::gate::{meter_level, rms};

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const PLAYBACK_PRIME_FRAMES: usize = 3;
const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
const LEVEL_INTERVAL: Duration = Duration::from_millis(50);

pub struct LoopbackOptions {
	pub input_device: Option<String>,
	pub output_device: Option<String>,
	pub gain: f32,
	pub denoise: bool,
}

impl Default for LoopbackOptions {
	fn default() -> Self {
		Self {
			input_device: None,
			output_device: None,
			gain: 1.0,
			denoise: true,
		}
	}
}

pub fn loopback(seconds: u32) -> Result<()> {
	loopback_with_levels(
		seconds,
		LoopbackOptions::default(),
		&AtomicBool::new(false),
		&|_| {},
	)
}

pub fn loopback_with_levels(
	seconds: u32,
	options: LoopbackOptions,
	cancel: &AtomicBool,
	on_level: &(dyn Fn(f32) + Send + Sync),
) -> Result<()> {
	let rings = Arc::new(Rings::new());
	let _engine = AudioEngine::start(
		rings.clone(),
		AudioRoute::Call,
		options.input_device,
		options.output_device,
	)?;
	if rings.capture_rate() == 0 {
		return Err(VoiceError::Audio("no input device".to_owned()));
	}
	if rings.playback_rate() == 0 {
		return Err(VoiceError::Audio("no output device".to_owned()));
	}
	let mut capture = CaptureChain::default();
	let mut playback = PlaybackChain::new(rings.clone());
	let mut encoder = VoiceEncoder::new(Profile::Voice)?;
	let mut decoder = VoiceDecoder::new()?;
	let mut denoiser = Denoiser::new();
	let mut microphone = vec![0.0; FRAME_SAMPLES];
	let mut speaker = vec![0.0; STEREO_FRAME_SAMPLES];
	for _ in 0..PLAYBACK_PRIME_FRAMES {
		playback.push(&speaker)?;
	}
	let deadline = Instant::now() + Duration::from_secs(u64::from(seconds));
	let mut next_level = Instant::now() + LEVEL_INTERVAL;
	let mut peak = 0.0f32;
	while Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
		capture.pull(&rings)?;
		while capture.take_frame(&mut microphone) {
			microphone.iter_mut().for_each(|sample| *sample *= options.gain);
			peak = peak.max(meter_level(rms(&microphone)));
			if options.denoise {
				denoiser.process(&mut microphone);
			}
			let packet = encoder.encode(&microphone)?;
			decoder.decode(packet, false, &mut speaker)?;
			playback.push(&speaker)?;
		}
		if Instant::now() >= next_level {
			on_level(peak);
			peak = 0.0;
			next_level += LEVEL_INTERVAL;
		}
		sleep(POLL_INTERVAL);
	}
	if cancel.load(Ordering::Relaxed) {
		rings.clear_playback();
		return Ok(());
	}
	let drain_deadline = Instant::now() + DRAIN_TIMEOUT;
	while rings.playback_len() > 0 && Instant::now() < drain_deadline {
		sleep(POLL_INTERVAL);
	}
	Ok(())
}
