use std::f32::consts::{PI, TAU};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Error, ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

const FADE_SECONDS: f32 = 0.02;
const ATTACK_SECONDS: f32 = 0.006;
const GLIDE_SECONDS: f32 = 0.025;
const GLIDE_SHARPNESS: f32 = 0.03;
const HARMONIC_GAIN: f32 = 0.1;
const HARMONIC_DECAY_RATIO: f32 = 3.;
const LOW_PASS_HERTZ: f32 = 1800.;
const PEAK_LEVEL: f32 = 0.2;
const PLAYBACK_SLACK: Duration = Duration::from_millis(120);

const fn note(hertz: f32, start: f32, decay_seconds: f32, gain: f32) -> Note {
	Note {
		hertz,
		start,
		decay_seconds,
		gain,
	}
}

const MESSAGE_NOTES: [Note; 2] = [note(392., 0., 0.07, 1.), note(523.25, 0.065, 0.11, 0.794)];
const JOIN_NOTES: [Note; 3] = [
	note(523.25, 0., 0.1, 1.),
	note(659.25, 0.08, 0.12, 0.9),
	note(783.99, 0.16, 0.16, 0.8),
];
const LEAVE_NOTES: [Note; 3] = [
	note(659.25, 0., 0.1, 1.),
	note(523.25, 0.09, 0.12, 0.9),
	note(392., 0.18, 0.16, 0.8),
];
const MUTE_NOTES: [Note; 2] = [note(440., 0., 0.06, 1.), note(330., 0.06, 0.09, 0.9)];
const UNMUTE_NOTES: [Note; 2] = [note(330., 0., 0.06, 1.), note(440., 0.06, 0.09, 0.9)];
const DEAFEN_NOTES: [Note; 3] = [
	note(349.23, 0., 0.06, 1.),
	note(261.63, 0.07, 0.08, 0.9),
	note(196., 0.14, 0.12, 0.8),
];
const UNDEAFEN_NOTES: [Note; 3] = [
	note(196., 0., 0.06, 1.),
	note(261.63, 0.07, 0.08, 0.9),
	note(349.23, 0.14, 0.12, 0.8),
];
const STREAM_START_NOTES: [Note; 3] = [
	note(440., 0., 0.08, 1.),
	note(587.33, 0.07, 0.1, 0.9),
	note(880., 0.14, 0.16, 0.8),
];
const STREAM_END_NOTES: [Note; 3] = [
	note(880., 0., 0.08, 1.),
	note(587.33, 0.07, 0.1, 0.9),
	note(440., 0.14, 0.16, 0.8),
];
const CAMERA_ON_NOTES: [Note; 2] = [note(587.33, 0., 0.05, 1.), note(783.99, 0.05, 0.09, 0.9)];
const CAMERA_OFF_NOTES: [Note; 2] = [note(783.99, 0., 0.05, 1.), note(587.33, 0.05, 0.09, 0.9)];
const RING_NOTES: [Note; 4] = [
	note(880., 0., 0.3, 1.),
	note(659.25, 0.18, 0.3, 0.9),
	note(880., 0.8, 0.3, 1.),
	note(659.25, 0.98, 0.3, 0.9),
];
const RINGBACK_NOTES: [Note; 4] = [
	note(440., 0., 0.4, 1.),
	note(480., 0., 0.4, 1.),
	note(440., 0.6, 0.4, 1.),
	note(480., 0.6, 0.4, 1.),
];

const MAX_PLAYING: usize = 4;

static PLAYING: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
	Message,
	Join,
	Leave,
	Mute,
	Unmute,
	Deafen,
	Undeafen,
	StreamStart,
	StreamEnd,
	CameraOn,
	CameraOff,
	Ring,
	Ringback,
}

impl Cue {
	fn notes(self) -> &'static [Note] {
		match self {
			Self::Message => &MESSAGE_NOTES,
			Self::Join => &JOIN_NOTES,
			Self::Leave => &LEAVE_NOTES,
			Self::Mute => &MUTE_NOTES,
			Self::Unmute => &UNMUTE_NOTES,
			Self::Deafen => &DEAFEN_NOTES,
			Self::Undeafen => &UNDEAFEN_NOTES,
			Self::StreamStart => &STREAM_START_NOTES,
			Self::StreamEnd => &STREAM_END_NOTES,
			Self::CameraOn => &CAMERA_ON_NOTES,
			Self::CameraOff => &CAMERA_OFF_NOTES,
			Self::Ring => &RING_NOTES,
			Self::Ringback => &RINGBACK_NOTES,
		}
	}

	fn seconds(self) -> f32 {
		match self {
			Self::Message => 0.38,
			Self::Join | Self::Leave | Self::Deafen | Self::Undeafen => 0.45,
			Self::Mute | Self::Unmute => 0.28,
			Self::StreamStart | Self::StreamEnd => 0.5,
			Self::CameraOn | Self::CameraOff => 0.25,
			Self::Ring => 2.2,
			Self::Ringback => 3.,
		}
	}
}

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

fn render(cue: Cue, rate: u32, volume: f32) -> Vec<f32> {
	let rate = rate as f32;
	let frames = (cue.seconds() * rate) as usize;
	let fade_frames = FADE_SECONDS * rate;
	let smoothing = 1. - (-TAU * LOW_PASS_HERTZ / rate).exp();
	let notes = cue.notes();
	let mut filtered = 0.;
	let mut samples: Vec<f32> = (0..frames)
		.map(|index| {
			let at = index as f32 / rate;
			filtered += smoothing * (notes.iter().map(|note| note.sample(at)).sum::<f32>() - filtered);
			let tail = ((frames - 1 - index) as f32 / fade_frames).min(1.);
			filtered * (0.5 - 0.5 * (PI * tail).cos())
		})
		.collect();
	let peak = samples.iter().fold(0f32, |peak, sample| peak.max(sample.abs()));
	if peak > 0. {
		let scale = PEAK_LEVEL * volume / peak;
		samples.iter_mut().for_each(|sample| *sample *= scale);
	}
	samples
}

struct Playback {
	samples: Vec<f32>,
	position: usize,
	repeat: bool,
}

impl Playback {
	fn next_sample(&mut self) -> f32 {
		if self.position >= self.samples.len() {
			if !self.repeat {
				return 0.;
			}
			self.position = 0;
		}
		let sample = self.samples[self.position];
		self.position += 1;
		sample
	}
}

pub fn play(cue: Cue, volume: f32) {
	let volume = volume.clamp(0., 1.);
	if volume == 0. {
		return;
	}
	let reserved = PLAYING.try_update(Ordering::AcqRel, Ordering::Acquire, |playing| {
		(playing < MAX_PLAYING).then_some(playing + 1)
	});
	if reserved.is_err() {
		return;
	}
	let spawned = thread::Builder::new()
		.name("oxidcord-sound".to_owned())
		.spawn(move || {
			match open_stream(cue, volume, false) {
				Ok(_stream) => thread::sleep(Duration::from_secs_f32(cue.seconds()) + PLAYBACK_SLACK),
				Err(error) => tracing::warn!(%error, "could not play the sound"),
			}
			PLAYING.fetch_sub(1, Ordering::AcqRel);
		});
	if let Err(error) = spawned {
		PLAYING.fetch_sub(1, Ordering::AcqRel);
		tracing::warn!(%error, "could not start the sound thread");
	}
}

pub struct Looping {
	_stop: Sender<()>,
}

pub fn start_loop(cue: Cue, volume: f32) -> Option<Looping> {
	let volume = volume.clamp(0., 1.);
	if volume == 0. {
		return None;
	}
	let (stop, stopped) = channel::<()>();
	let spawned = thread::Builder::new()
		.name("oxidcord-sound-loop".to_owned())
		.spawn(move || match open_stream(cue, volume, true) {
			Ok(_stream) => {
				let _ = stopped.recv();
			}
			Err(error) => tracing::warn!(%error, "could not loop the sound"),
		});
	match spawned {
		Ok(_) => Some(Looping { _stop: stop }),
		Err(error) => {
			tracing::warn!(%error, "could not start the sound loop thread");
			None
		}
	}
}

fn open_stream(cue: Cue, volume: f32, repeat: bool) -> Result<Stream, Error> {
	let device = cpal::default_host()
		.default_output_device()
		.ok_or_else(|| Error::with_message(ErrorKind::DeviceNotAvailable, "no output device"))?;
	let supported = device.default_output_config()?;
	let config = supported.config();
	let stream = match supported.sample_format() {
		SampleFormat::F32 => cue_stream::<f32>(&device, config, cue, volume, repeat),
		SampleFormat::I16 => cue_stream::<i16>(&device, config, cue, volume, repeat),
		SampleFormat::U16 => cue_stream::<u16>(&device, config, cue, volume, repeat),
		SampleFormat::I32 => cue_stream::<i32>(&device, config, cue, volume, repeat),
		format => Err(Error::with_message(
			ErrorKind::UnsupportedConfig,
			format!("sample format {format}"),
		)),
	}?;
	stream.play()?;
	Ok(stream)
}

fn cue_stream<T>(
	device: &Device,
	config: StreamConfig,
	cue: Cue,
	volume: f32,
	repeat: bool,
) -> Result<Stream, Error>
where
	T: SizedSample + FromSample<f32>,
{
	let channels = usize::from(config.channels);
	let mut playback = Playback {
		samples: render(cue, config.sample_rate, volume),
		position: 0,
		repeat,
	};
	device.build_output_stream(
		config,
		move |data: &mut [T], _| {
			for frame in data.chunks_exact_mut(channels) {
				frame.fill(T::from_sample(playback.next_sample()));
			}
		},
		|error| tracing::warn!(%error, "sound stream error"),
		None,
	)
}
