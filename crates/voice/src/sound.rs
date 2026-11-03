use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as FormatError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::packet::Packet;
use symphonia::default::{get_codecs, get_probe};

use crate::codec::{SAMPLE_RATE, STEREO_FRAME_SAMPLES};
use crate::error::{Result, VoiceError};
use crate::mixer::Mixer;
use crate::resample::Resampler;

const MAX_SOUND_SECONDS: usize = 12;
const MAX_BAD_PACKETS: usize = 8;
const STEREO: usize = 2;
const OPUS_MAX_FRAMES: usize = 5760;
const OPUS_HEAD_CHANNELS: usize = 9;
const OPUS_HEAD_PRE_SKIP: usize = 10;

#[derive(Debug, Clone)]
pub struct Sound {
	samples: Arc<[f32]>,
}

impl Sound {
	pub fn duration(&self) -> Duration {
		Duration::from_secs_f64(self.samples.len() as f64 / (STEREO as f64 * f64::from(SAMPLE_RATE)))
	}
}

pub(crate) struct Playing {
	sound: Sound,
	position: usize,
	gain: f32,
}

impl Playing {
	pub fn new(sound: Sound, gain: f32) -> Self {
		Self {
			sound,
			position: 0,
			gain,
		}
	}

	pub fn mix_into(&mut self, mixer: &mut Mixer) -> bool {
		let samples = &self.sound.samples;
		let end = (self.position + STEREO_FRAME_SAMPLES).min(samples.len());
		mixer.add(&samples[self.position..end], self.gain);
		self.position = end;
		end < samples.len()
	}
}

enum Codec {
	Generic(Box<dyn AudioDecoder>),
	Opus {
		decoder: opus::Decoder,
		channels: usize,
		skip: usize,
	},
}

struct Decoded {
	rate: u32,
	stereo: Vec<f32>,
}

fn sound_error(message: impl ToString) -> VoiceError {
	VoiceError::Audio(message.to_string())
}

fn open_codec(params: &CodecParameters) -> Result<(Codec, u32)> {
	let CodecParameters::Audio(params) = params else {
		return Err(sound_error("not an audio track"));
	};
	if params.codec == CODEC_ID_OPUS {
		return open_opus(params);
	}
	let rate = params
		.sample_rate
		.ok_or_else(|| sound_error("the sound has no sample rate"))?;
	let decoder = get_codecs()
		.make_audio_decoder(params, &AudioDecoderOptions::default())
		.map_err(sound_error)?;
	Ok((Codec::Generic(decoder), rate))
}

fn open_opus(params: &AudioCodecParameters) -> Result<(Codec, u32)> {
	let head = params.extra_data.as_deref().unwrap_or_default();
	let channels = head
		.get(OPUS_HEAD_CHANNELS)
		.map(|count| usize::from(*count))
		.or_else(|| params.channels.as_ref().map(|channels| channels.count()))
		.unwrap_or(STEREO);
	let layout = match channels {
		1 => opus::Channels::Mono,
		2 => opus::Channels::Stereo,
		other => return Err(sound_error(format!("opus with {other} channels"))),
	};
	let skip = head
		.get(OPUS_HEAD_PRE_SKIP..OPUS_HEAD_PRE_SKIP + 2)
		.map_or(0, |bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]])));
	let decoder = opus::Decoder::new(SAMPLE_RATE, layout)?;
	Ok((
		Codec::Opus {
			decoder,
			channels,
			skip,
		},
		SAMPLE_RATE,
	))
}

fn push_stereo(interleaved: &[f32], channels: usize, stereo: &mut Vec<f32>) {
	match channels {
		1 => stereo.extend(interleaved.iter().flat_map(|sample| [*sample, *sample])),
		2 => stereo.extend_from_slice(interleaved),
		_ => stereo.extend(
			interleaved
				.chunks_exact(channels)
				.flat_map(|frame| [frame[0], frame[1]]),
		),
	}
}

fn decode_packet(codec: &mut Codec, packet: &Packet, out: &mut Vec<f32>) -> Result<()> {
	match codec {
		Codec::Generic(decoder) => {
			let buffer = decoder.decode(packet).map_err(sound_error)?;
			let channels = buffer.spec().channels().count().max(1);
			let mut interleaved = Vec::new();
			buffer.copy_to_vec_interleaved(&mut interleaved);
			push_stereo(&interleaved, channels, out);
		}
		Codec::Opus {
			decoder,
			channels,
			skip,
		} => {
			let mut interleaved = vec![0.0; OPUS_MAX_FRAMES * *channels];
			let frames = decoder.decode_float(&packet.data, &mut interleaved, false)?;
			let dropped = (*skip).min(frames);
			*skip -= dropped;
			push_stereo(
				&interleaved[dropped * *channels..frames * *channels],
				*channels,
				out,
			);
		}
	}
	Ok(())
}

fn decode_all(bytes: Vec<u8>) -> Result<Decoded> {
	let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), MediaSourceStreamOptions::default());
	let mut format = get_probe()
		.probe(
			&Hint::new(),
			stream,
			FormatOptions::default(),
			MetadataOptions::default(),
		)
		.map_err(sound_error)?;
	let track = format
		.default_track(TrackType::Audio)
		.ok_or_else(|| sound_error("the sound has no audio track"))?;
	let track_id = track.id;
	let params = track
		.codec_params
		.as_ref()
		.ok_or_else(|| sound_error("the sound has no codec parameters"))?;
	let (mut codec, rate) = open_codec(params)?;
	let limit = MAX_SOUND_SECONDS * rate as usize * STEREO;
	let mut stereo = Vec::new();
	let mut bad = 0;
	while stereo.len() < limit {
		let packet = match format.next_packet() {
			Ok(Some(packet)) => packet,
			Ok(None) => break,
			Err(FormatError::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
			Err(error) => return Err(sound_error(error)),
		};
		if packet.track_id != track_id {
			continue;
		}
		if let Err(error) = decode_packet(&mut codec, &packet, &mut stereo) {
			bad += 1;
			if bad > MAX_BAD_PACKETS {
				return Err(error);
			}
		}
	}
	Ok(Decoded { rate, stereo })
}

pub fn decode_sound(bytes: Vec<u8>) -> Result<Sound> {
	let Decoded { rate, stereo } = decode_all(bytes)?;
	if stereo.is_empty() {
		return Err(sound_error("the sound is empty"));
	}
	let mut converted = Vec::new();
	Resampler::new(rate, SAMPLE_RATE, STEREO)?.process(&stereo, &mut converted)?;
	Ok(Sound {
		samples: converted.into(),
	})
}
