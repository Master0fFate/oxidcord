use sonora::config::{AdaptiveDigital, EchoCanceller, GainController2};
use sonora::{AudioProcessing, Config, StreamConfig};
use tracing::warn;

use crate::codec::{FRAME_SAMPLES, SAMPLE_RATE};

const CHUNK_SAMPLES: usize = FRAME_SAMPLES / 2;
const STEREO_CHUNK_SAMPLES: usize = CHUNK_SAMPLES * 2;
const MONO: u16 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Stages {
	pub echo_cancellation: bool,
	pub auto_gain: bool,
}

impl Stages {
	fn config(self) -> Config {
		Config {
			echo_canceller: self.echo_cancellation.then(EchoCanceller::default),
			gain_controller2: self.auto_gain.then(|| GainController2 {
				adaptive_digital: Some(AdaptiveDigital::default()),
				..GainController2::default()
			}),
			..Config::default()
		}
	}
}

pub struct VoiceProcessor {
	apm: AudioProcessing,
	stages: Stages,
	input: [f32; CHUNK_SAMPLES],
	reference: [f32; CHUNK_SAMPLES],
	discarded: [f32; CHUNK_SAMPLES],
}

impl VoiceProcessor {
	pub fn new() -> Self {
		let stream = StreamConfig::new(SAMPLE_RATE, MONO);
		Self {
			apm: AudioProcessing::builder()
				.capture_config(stream)
				.render_config(stream)
				.build(),
			stages: Stages::default(),
			input: [0.0; CHUNK_SAMPLES],
			reference: [0.0; CHUNK_SAMPLES],
			discarded: [0.0; CHUNK_SAMPLES],
		}
	}

	pub fn set_stages(&mut self, stages: Stages) {
		if stages != self.stages {
			self.stages = stages;
			self.apm.apply_config(stages.config());
		}
	}

	pub fn analyze_playback(&mut self, stereo: &[f32]) {
		if !self.stages.echo_cancellation {
			return;
		}
		for chunk in stereo.as_chunks::<STEREO_CHUNK_SAMPLES>().0 {
			for (mono, pair) in self.reference.iter_mut().zip(chunk.as_chunks::<2>().0) {
				*mono = (pair[0] + pair[1]) * 0.5;
			}
			if let Err(error) = self
				.apm
				.process_render_f32(&[&self.reference], &mut [&mut self.discarded])
			{
				warn!(?error, "playback reference was rejected");
			}
		}
	}

	pub fn process_capture(&mut self, frame: &mut [f32]) {
		if self.stages == Stages::default() {
			return;
		}
		for chunk in frame.as_chunks_mut::<CHUNK_SAMPLES>().0 {
			self.input = *chunk;
			if let Err(error) = self
				.apm
				.process_capture_f32(&[&self.input], &mut [chunk.as_mut_slice()])
			{
				warn!(?error, "microphone processing failed");
			}
		}
	}
}
