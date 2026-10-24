use std::f32::consts::{PI, TAU};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Error, ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

const CHIME_SECONDS: f32 = 0.38;
const FADE_SECONDS: f32 = 0.02;
const ATTACK_SECONDS: f32 = 0.006;
const GLIDE_SECONDS: f32 = 0.025;
const GLIDE_SHARPNESS: f32 = 0.03;
const HARMONIC_GAIN: f32 = 0.1;
const HARMONIC_DECAY_RATIO: f32 = 3.;
const LOW_PASS_HERTZ: f32 = 1800.;
const PEAK_LEVEL: f32 = 0.2;
const PLAYBACK_SLACK: Duration = Duration::from_millis(120);
const NOTES: [Note; 2] = [
	Note {
		hertz: 392.,
		start: 0.,
		decay_seconds: 0.07,
		gain: 1.,
	},
	Note {
		hertz: 523.25,
		start: 0.065,
		decay_seconds: 0.11,
		gain: 0.794,
	},
];

static PLAYING: AtomicBool = AtomicBool::new(false);

struct Note {
	hertz: f32,
	start: f32,
	decay_seconds: f32,
	gain: f32,
}

impl Note {
	fn sample(&self, at: f32) -> f32 {
		let local = at - self.start;
		if local < 0. {
			return 0.;
		}
		let attack = if local < ATTACK_SECONDS {
			0.5 - 0.5 * (PI * local / ATTACK_SECONDS).cos()
		} else {
			1.
		};
		let remaining = (1. - local / GLIDE_SECONDS).max(0.);
		let glide = GLIDE_SECONDS / 3. * (1. - remaining.powi(3));
		let phase = TAU * self.hertz * (local + GLIDE_SHARPNESS * glide);
		let fundamental = (-local / self.decay_seconds).exp() * phase.sin();
		let harmonic =
			HARMONIC_GAIN * (-local * HARMONIC_DECAY_RATIO / self.decay_seconds).exp() * (2. * phase).sin();
		self.gain * attack * (fundamental + harmonic)
	}
}

fn render_chime(rate: u32) -> Vec<f32> {
	let rate = rate as f32;
	let frames = (CHIME_SECONDS * rate) as usize;
	let fade_frames = FADE_SECONDS * rate;
	let smoothing = 1. - (-TAU * LOW_PASS_HERTZ / rate).exp();
	let mut filtered = 0.;
	let mut samples: Vec<f32> = (0..frames)
		.map(|index| {
			let at = index as f32 / rate;
			filtered += smoothing * (NOTES.iter().map(|note| note.sample(at)).sum::<f32>() - filtered);
			let tail = ((frames - 1 - index) as f32 / fade_frames).min(1.);
			filtered * (0.5 - 0.5 * (PI * tail).cos())
		})
		.collect();
	let peak = samples.iter().fold(0f32, |peak, sample| peak.max(sample.abs()));
	if peak > 0. {
		let scale = PEAK_LEVEL / peak;
		samples.iter_mut().for_each(|sample| *sample *= scale);
	}
	samples
}

pub fn play_chime() {
	if PLAYING.swap(true, Ordering::AcqRel) {
		return;
	}
	let spawned = thread::Builder::new()
		.name("oxidcord-chime".to_owned())
		.spawn(|| {
			match open_stream() {
				Ok(_stream) => thread::sleep(Duration::from_secs_f32(CHIME_SECONDS) + PLAYBACK_SLACK),
				Err(error) => tracing::warn!(%error, "could not play the notification sound"),
			}
			PLAYING.store(false, Ordering::Release);
		});
	if let Err(error) = spawned {
		PLAYING.store(false, Ordering::Release);
		tracing::warn!(%error, "could not start the notification sound thread");
	}
}

fn open_stream() -> Result<Stream, Error> {
	let device = cpal::default_host()
		.default_output_device()
		.ok_or_else(|| Error::with_message(ErrorKind::DeviceNotAvailable, "no output device"))?;
	let supported = device.default_output_config()?;
	let config = supported.config();
	let stream = match supported.sample_format() {
		SampleFormat::F32 => chime_stream::<f32>(&device, config),
		SampleFormat::I16 => chime_stream::<i16>(&device, config),
		SampleFormat::U16 => chime_stream::<u16>(&device, config),
		SampleFormat::I32 => chime_stream::<i32>(&device, config),
		format => Err(Error::with_message(
			ErrorKind::UnsupportedConfig,
			format!("sample format {format}"),
		)),
	}?;
	stream.play()?;
	Ok(stream)
}

fn chime_stream<T>(device: &Device, config: StreamConfig) -> Result<Stream, Error>
where
	T: SizedSample + FromSample<f32>,
{
	let channels = usize::from(config.channels);
	let mut samples = render_chime(config.sample_rate).into_iter();
	device.build_output_stream(
		config,
		move |data: &mut [T], _| {
			for frame in data.chunks_exact_mut(channels) {
				frame.fill(T::from_sample(samples.next().unwrap_or(0.)));
			}
		},
		|error| tracing::warn!(%error, "notification sound stream error"),
		None,
	)
}
