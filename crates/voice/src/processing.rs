use sonora::config::{AdaptiveDigital, EchoCanceller, GainController2};
use sonora::{AudioProcessing, Config, StreamConfig};
use tracing::warn;

use crate::codec::{FRAME_SAMPLES, SAMPLE_RATE};
use crate::render_tap::RenderReader;

const CHUNK_SAMPLES: usize = FRAME_SAMPLES / 2;
const MONO: u16 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Stages {
	pub echo_cancellation: bool,
	pub auto_gain: bool,
}

impl Stages {
	fn echo_config(self) -> Config {
		Config {
			echo_canceller: self.echo_cancellation.then(EchoCanceller::default),
			..Config::default()
		}
	}

	fn gain_config(self) -> Config {
		Config {
			gain_controller2: self.auto_gain.then(|| GainController2 {
				adaptive_digital: Some(AdaptiveDigital::default()),
				..GainController2::default()
			}),
			..Config::default()
		}
	}
}

fn build() -> AudioProcessing {
	let stream = StreamConfig::new(SAMPLE_RATE, MONO);
	AudioProcessing::builder()
		.capture_config(stream)
		.render_config(stream)
		.build()
}

fn report_once(reported: &mut bool, stage: &'static str, error: impl std::fmt::Debug) {
	if !std::mem::replace(reported, true) {
		warn!(?error, stage, "audio processing failed");
	}
}

fn process_chunk(
	apm: &mut AudioProcessing,
	input: &mut [f32; CHUNK_SAMPLES],
	chunk: &mut [f32; CHUNK_SAMPLES],
	reported: &mut bool,
	stage: &'static str,
) {
	*input = *chunk;
	if let Err(error) = apm.process_capture_f32(&[&*input], &mut [chunk.as_mut_slice()]) {
		report_once(reported, stage, error);
	}
}

pub struct VoiceProcessor {
	echo: AudioProcessing,
	gain: AudioProcessing,
	stages: Stages,
	far_end: RenderReader,
	input: [f32; CHUNK_SAMPLES],
	reference: [f32; CHUNK_SAMPLES],
	discarded: [f32; CHUNK_SAMPLES],
	reported: bool,
}

impl VoiceProcessor {
	pub fn new() -> Self {
		Self {
			echo: build(),
			gain: build(),
			stages: Stages::default(),
			far_end: RenderReader::default(),
			input: [0.0; CHUNK_SAMPLES],
			reference: [0.0; CHUNK_SAMPLES],
			discarded: [0.0; CHUNK_SAMPLES],
			reported: false,
		}
	}

	pub fn set_stages(&mut self, stages: Stages) {
		if stages != self.stages {
			self.stages = stages;
			self.reported = false;
			self.echo.apply_config(stages.echo_config());
			self.gain.apply_config(stages.gain_config());
		}
	}

	pub fn cancel_echo(&mut self, frame: &mut [f32], backlog: usize) {
		if !self.stages.echo_cancellation {
			return;
		}
		let chunks = frame.as_chunks_mut::<CHUNK_SAMPLES>().0;
		let count = chunks.len();
		for (index, chunk) in chunks.iter_mut().enumerate() {
			self.far_end
				.read(backlog + (count - 1 - index) * CHUNK_SAMPLES, &mut self.reference);
			if let Err(error) = self
				.echo
				.process_render_f32(&[&self.reference], &mut [&mut self.discarded])
			{
				report_once(&mut self.reported, "playback reference", error);
			}
			process_chunk(
				&mut self.echo,
				&mut self.input,
				chunk,
				&mut self.reported,
				"echo cancellation",
			);
		}
	}

	pub fn level_gain(&mut self, frame: &mut [f32]) {
		if !self.stages.auto_gain {
			return;
		}
		for chunk in frame.as_chunks_mut::<CHUNK_SAMPLES>().0 {
			process_chunk(
				&mut self.gain,
				&mut self.input,
				chunk,
				&mut self.reported,
				"automatic gain",
			);
		}
	}
}
