use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known::{
	CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_OPUS, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::packet::Packet;
use symphonia::default::get_codecs;

use crate::error::{PlayerError, Result};
use crate::output::OUTPUT_CHANNELS;
use crate::video::FfAudioDecoder;

const OPUS_RATE: u32 = 48_000;
const OPUS_MAX_FRAMES: usize = 5760;
const OPUS_HEAD_CHANNELS: usize = 9;
const OPUS_HEAD_PRE_SKIP: usize = 10;

enum Decoder {
	Generic(Box<dyn AudioDecoder>),
	Opus {
		decoder: opus::Decoder,
		channels: usize,
		pre_skip: usize,
		skip: usize,
	},
	Pack(FfAudioDecoder),
}

pub struct AudioTrack {
	decoder: Decoder,
	rate: u32,
	decoded: Vec<f32>,
	stereo: Vec<f32>,
}

impl AudioTrack {
	pub fn open(params: &CodecParameters) -> Result<Self> {
		let CodecParameters::Audio(params) = params else {
			return Err(PlayerError::Unsupported("not an audio track".to_owned()));
		};
		if params.codec == CODEC_ID_OPUS {
			return Self::opus(params);
		}
		let rate = params
			.sample_rate
			.ok_or_else(|| PlayerError::Format("audio track has no sample rate".to_owned()))?;
		let decoder = match get_codecs().make_audio_decoder(params, &AudioDecoderOptions::default()) {
			Ok(decoder) => decoder,
			Err(error) => {
				tracing::debug!(%error, "symphonia audio decoder unavailable");
				return Self::pack(params, rate);
			}
		};
		Ok(Self {
			decoder: Decoder::Generic(decoder),
			rate,
			decoded: Vec::new(),
			stereo: Vec::new(),
		})
	}

	fn pack(params: &AudioCodecParameters, rate: u32) -> Result<Self> {
		let name = match params.codec {
			CODEC_ID_AAC => c"aac",
			CODEC_ID_MP3 => c"mp3",
			CODEC_ID_VORBIS => c"vorbis",
			CODEC_ID_FLAC => c"flac",
			CODEC_ID_ALAC => c"alac",
			other => return Err(PlayerError::Unsupported(format!("audio codec {other}"))),
		};
		let channels = params
			.channels
			.as_ref()
			.map_or(OUTPUT_CHANNELS, |channels| channels.count());
		Ok(Self {
			decoder: Decoder::Pack(FfAudioDecoder::open(
				name,
				channels,
				params.extra_data.as_deref(),
			)?),
			rate: aac_output_rate(params).unwrap_or(rate),
			decoded: Vec::new(),
			stereo: Vec::new(),
		})
	}

	fn opus(params: &AudioCodecParameters) -> Result<Self> {
		let head = params.extra_data.as_deref().unwrap_or_default();
		let channels = head
			.get(OPUS_HEAD_CHANNELS)
			.map(|count| usize::from(*count))
			.or_else(|| params.channels.as_ref().map(|channels| channels.count()))
			.unwrap_or(OUTPUT_CHANNELS);
		let layout = match channels {
			1 => opus::Channels::Mono,
			2 => opus::Channels::Stereo,
			other => return Err(PlayerError::Unsupported(format!("opus with {other} channels"))),
		};
		let pre_skip = head
			.get(OPUS_HEAD_PRE_SKIP..OPUS_HEAD_PRE_SKIP + 2)
			.map_or(0, |bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]])));
		let decoder =
			opus::Decoder::new(OPUS_RATE, layout).map_err(|error| PlayerError::Audio(error.to_string()))?;
		Ok(Self {
			decoder: Decoder::Opus {
				decoder,
				channels,
				pre_skip,
				skip: pre_skip,
			},
			rate: OPUS_RATE,
			decoded: vec![0.0; OPUS_MAX_FRAMES * channels],
			stereo: Vec::new(),
		})
	}

	pub fn rate(&self) -> u32 {
		self.rate
	}

	pub fn reset(&mut self, from_start: bool) {
		match &mut self.decoder {
			Decoder::Generic(decoder) => decoder.reset(),
			Decoder::Pack(decoder) => decoder.reset(),
			Decoder::Opus {
				decoder,
				pre_skip,
				skip,
				..
			} => {
				let _ = decoder.reset_state();
				*skip = if from_start { *pre_skip } else { 0 };
			}
		}
	}

	pub fn decode(&mut self, packet: &Packet) -> Result<&[f32]> {
		let (channels, skip) = match &mut self.decoder {
			Decoder::Generic(decoder) => {
				let buffer = decoder.decode(packet)?;
				let channels = buffer.spec().channels().count();
				buffer.copy_to_vec_interleaved(&mut self.decoded);
				(channels, 0)
			}
			Decoder::Opus {
				decoder,
				channels,
				skip,
				..
			} => {
				let frames = decoder
					.decode_float(&packet.data, &mut self.decoded, false)
					.map_err(|error| PlayerError::Audio(error.to_string()))?;
				let dropped = (*skip).min(frames);
				*skip -= dropped;
				self.stereo.clear();
				stereo_into(
					&self.decoded[dropped * *channels..frames * *channels],
					*channels,
					&mut self.stereo,
				);
				return Ok(&self.stereo);
			}
			Decoder::Pack(decoder) => {
				self.decoded.clear();
				decoder.decode(&packet.data, &mut self.decoded)?;
				self.stereo.clear();
				stereo_into(&self.decoded, decoder.channels(), &mut self.stereo);
				return Ok(&self.stereo);
			}
		};
		self.stereo.clear();
		stereo_into(&self.decoded[skip..], channels.max(1), &mut self.stereo);
		Ok(&self.stereo)
	}
}

fn stereo_into(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
	out.reserve(interleaved.len() / channels * OUTPUT_CHANNELS);
	match channels {
		1 => out.extend(interleaved.iter().flat_map(|sample| [*sample, *sample])),
		2 => out.extend_from_slice(interleaved),
		_ => out.extend(
			interleaved
				.chunks_exact(channels)
				.flat_map(|frame| [frame[0], frame[1]]),
		),
	}
}

const AAC_SAMPLE_RATES: [u32; 13] = [
	96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];
const AAC_EXPLICIT_RATE: u32 = 0x0f;
const AAC_SBR: u32 = 5;
const AAC_PS: u32 = 29;
const AAC_EXTENDED_TYPE: u32 = 31;

struct Bits<'a> {
	data: &'a [u8],
	at: usize,
}

impl Bits<'_> {
	fn read(&mut self, count: usize) -> Option<u32> {
		let mut value = 0u32;
		for _ in 0..count {
			let byte = *self.data.get(self.at / 8)?;
			value = (value << 1) | u32::from((byte >> (7 - self.at % 8)) & 1);
			self.at += 1;
		}
		Some(value)
	}

	fn sample_rate(&mut self) -> Option<u32> {
		match self.read(4)? {
			AAC_EXPLICIT_RATE => self.read(24),
			index => AAC_SAMPLE_RATES.get(index as usize).copied(),
		}
	}
}

fn aac_output_rate(params: &AudioCodecParameters) -> Option<u32> {
	if params.codec != CODEC_ID_AAC {
		return None;
	}
	let mut bits = Bits {
		data: params.extra_data.as_deref()?,
		at: 0,
	};
	let kind = bits.read(5)?;
	let kind = if kind == AAC_EXTENDED_TYPE {
		bits.read(6)? + 32
	} else {
		kind
	};
	let rate = bits.sample_rate()?;
	(kind == AAC_SBR || kind == AAC_PS).then_some(rate)
}
