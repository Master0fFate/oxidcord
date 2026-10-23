use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, warn};

use crate::chain::{CaptureChain, PlaybackChain};
use crate::codec::{FRAME_SAMPLES, SILENCE_FRAME, STEREO_FRAME_SAMPLES, VoiceEncoder};
use crate::crypto::PacketCipher;
use crate::denoise::Denoiser;
use crate::gate::{VoiceGate, meter_level, rms, to_db};
use crate::jitter::Push;
use crate::mixer::{Mixer, soft_clip};
use crate::remote::{Frame, RemoteStream};
use crate::rtp;
use crate::shared::{Control, InputMode, Shared};
use crate::transform::{Codec, FrameTransform};
use crate::video::VideoStream;
use crate::video::receive::{DecodeJob, DecodeWorker};
use crate::video::rtp::{self as video_rtp, H264Depacketizer};

const TICK: Duration = Duration::from_millis(10);
const FRAME_DURATION: Duration = Duration::from_millis(20);
const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(1);
const STREAM_TTL: Duration = Duration::from_secs(10);
const SILENCE_FRAMES_BEFORE_PAUSE: u8 = 5;
const MAX_DTX_PACKET_BYTES: usize = 2;
const MAX_FRAMES_PER_TICK: usize = 6;
const MAX_PLAYOUT_LAG: Duration = Duration::from_millis(100);
const METER_DECAY: f32 = 0.85;
const SYSTEM_ACTIVITY_DB: f32 = -60.0;
const RECEIVE_BUFFER_BYTES: usize = 2048;
const KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
const PSFB_PAYLOAD_TYPE: u8 = 206;
const PLI_FORMAT: u8 = 1;
const FIR_FORMAT: u8 = 4;
const RTCP_VERSION_2: u8 = 0x80;
const RTCP_FORMAT_MASK: u8 = 0x1f;
const PLI_LENGTH_WORDS: u16 = 2;

pub(crate) type SharedTransform = Arc<Mutex<Box<dyn FrameTransform>>>;

pub(crate) struct Transport {
	pub socket: Arc<UdpSocket>,
	pub ssrc: u32,
	pub cipher: PacketCipher,
}

struct VideoReceiver {
	depacketizer: H264Depacketizer,
	last_request: Option<Instant>,
	last_packet: Instant,
}

pub(crate) struct Media {
	transport: Transport,
	shared: Arc<Shared>,
	control: mpsc::UnboundedSender<Control>,
	transform: SharedTransform,
	audio_io: bool,
	capture: CaptureChain,
	system: CaptureChain,
	system_frame: Vec<f32>,
	playback: PlaybackChain,
	encoder: VoiceEncoder,
	denoiser: Denoiser,
	gate: VoiceGate,
	mixer: Mixer,
	streams: HashMap<u32, RemoteStream>,
	video_streams: HashMap<u32, VideoReceiver>,
	decoder: Option<DecodeWorker>,
	video_sequence: u16,
	frame: Vec<f32>,
	sequence: u16,
	timestamp: u32,
	speaking: bool,
	silence_left: u8,
	next_playout: Instant,
	#[cfg(feature = "testing")]
	next_injection: Instant,
}

impl Media {
	pub fn new(
		transport: Transport,
		shared: Arc<Shared>,
		control: mpsc::UnboundedSender<Control>,
		transform: SharedTransform,
		audio_io: bool,
	) -> crate::error::Result<Self> {
		let now = Instant::now();
		Ok(Self {
			transport,
			playback: PlaybackChain::new(shared.rings.clone()),
			shared,
			control,
			transform,
			audio_io,
			capture: CaptureChain::default(),
			system: CaptureChain::system(),
			system_frame: vec![0.0; FRAME_SAMPLES],
			encoder: VoiceEncoder::new()?,
			denoiser: Denoiser::new(),
			gate: VoiceGate::default(),
			mixer: Mixer::new(STEREO_FRAME_SAMPLES),
			streams: HashMap::new(),
			video_streams: HashMap::new(),
			decoder: None,
			video_sequence: rand::random(),
			frame: vec![0.0; FRAME_SAMPLES],
			sequence: rand::random(),
			timestamp: rand::random(),
			speaking: false,
			silence_left: 0,
			next_playout: now,
			#[cfg(feature = "testing")]
			next_injection: now,
		})
	}

	pub async fn run(mut self) {
		let socket = self.transport.socket.clone();
		let shared = self.shared.clone();
		let mut ticker = interval(TICK);
		ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
		let mut housekeeping = Instant::now();
		let mut buffer = [0u8; RECEIVE_BUFFER_BYTES];
		loop {
			tokio::select! {
				received = socket.recv(&mut buffer) => match received {
					Ok(length) => self.on_packet(&buffer[..length], Instant::now()),
					Err(error) if matches!(error.kind(), ErrorKind::ConnectionReset | ErrorKind::ConnectionRefused) => {}
					Err(error) => {
						warn!(%error, "voice udp receive failed");
						return;
					}
				},
				() = shared.video.outgoing_ready.notified() => self.send_video_frames().await,
				_ = ticker.tick() => {
					let now = Instant::now();
					self.capture_tick(now);
					self.playout_tick(now);
					if now.duration_since(housekeeping) >= HOUSEKEEPING_INTERVAL {
						housekeeping = now;
						self.housekeeping(now);
					}
				}
			}
		}
	}

	fn capture_tick(&mut self, now: Instant) {
		if self.audio_io
			&& let Err(error) = self.capture.pull(&self.shared.rings)
		{
			warn!(%error, "capture resampling failed");
		}
		#[cfg(feature = "testing")]
		self.inject(now);
		#[cfg(not(feature = "testing"))]
		let _ = now;
		let mut frame = std::mem::take(&mut self.frame);
		while self.capture.take_frame(&mut frame) {
			self.send_frame(&mut frame);
		}
		self.frame = frame;
	}

	#[cfg(feature = "testing")]
	fn inject(&mut self, now: Instant) {
		if now < self.next_injection {
			return;
		}
		if let Some(frame) = self.shared.test_input.lock().pop_front() {
			self.capture.inject(&frame);
			self.next_injection =
				(self.next_injection + FRAME_DURATION).max(now.checked_sub(MAX_PLAYOUT_LAG).unwrap_or(now));
		}
	}

	fn send_frame(&mut self, frame: &mut [f32]) {
		let gain = self.shared.input_volume.load();
		for sample in frame.iter_mut() {
			*sample *= gain;
		}
		let input_level = meter_level(rms(frame));
		let previous = self.shared.input_level.load();
		self.shared
			.input_level
			.store(input_level.max(previous * METER_DECAY));
		if self.shared.noise_suppression.load(Ordering::Relaxed) {
			self.denoiser.process(frame);
		}
		let microphone_active = self.should_transmit(to_db(rms(frame)));
		let system_active = self.mix_system(frame, microphone_active);
		if microphone_active || system_active {
			self.silence_left = SILENCE_FRAMES_BEFORE_PAUSE;
			if !self.speaking {
				self.speaking = true;
				let _ = self.control.send(Control::Speaking(true));
			}
			match self.encoder.encode(frame) {
				Ok(packet) if packet.len() > MAX_DTX_PACKET_BYTES => {
					let packet = packet.to_vec();
					self.send_audio(&packet);
				}
				Ok(_) => {}
				Err(error) => warn!(%error, "opus encode failed"),
			}
		} else if self.speaking {
			if self.silence_left > 0 {
				self.silence_left -= 1;
				self.send_silence();
			}
			if self.silence_left == 0 {
				self.speaking = false;
				let _ = self.control.send(Control::Speaking(false));
			}
		}
		self.timestamp = self.timestamp.wrapping_add(FRAME_SAMPLES as u32);
	}

	fn mix_system(&mut self, frame: &mut [f32], microphone_active: bool) -> bool {
		if let Err(error) = self.system.pull(&self.shared.rings) {
			warn!(%error, "system audio resampling failed");
		}
		if !self.system.take_frame(&mut self.system_frame) {
			return false;
		}
		if !microphone_active {
			frame.fill(0.0);
		}
		for (sample, system) in frame.iter_mut().zip(&self.system_frame) {
			*sample = soft_clip(*sample + system);
		}
		to_db(rms(&self.system_frame)) > SYSTEM_ACTIVITY_DB
	}

	fn should_transmit(&mut self, level_db: f32) -> bool {
		if self.shared.muted.load(Ordering::Relaxed) || self.shared.deafened.load(Ordering::Relaxed) {
			self.gate.reset();
			return false;
		}
		match self.shared.input_mode() {
			InputMode::VoiceActivity => self.gate.update(level_db, self.shared.vad_threshold_db.load()),
			InputMode::PushToTalk => self.shared.transmitting.load(Ordering::Relaxed),
		}
	}

	fn send_audio(&mut self, opus: &[u8]) {
		let Some(payload) = self.transform.lock().encrypt(Codec::Opus, opus) else {
			return;
		};
		self.send_payload(&payload);
	}

	fn send_silence(&mut self) {
		self.send_payload(&SILENCE_FRAME);
	}

	fn send_payload(&mut self, payload: &[u8]) {
		let header = rtp::write_header(self.sequence, self.timestamp, self.transport.ssrc);
		let Some(packet) = self.transport.cipher.seal(&header, payload) else {
			warn!("failed to encrypt a voice packet");
			return;
		};
		self.sequence = self.sequence.wrapping_add(1);
		if self.transport.socket.try_send(&packet).is_ok() {
			self.shared.counters.sent.fetch_add(1, Ordering::Relaxed);
		}
	}

	fn on_packet(&mut self, data: &[u8], now: Instant) {
		if rtp::is_rtcp(data) {
			self.on_rtcp(data);
			return;
		}
		let Some(header) = rtp::parse(data) else {
			return;
		};
		if header.payload_type == video_rtp::PAYLOAD_TYPE {
			self.on_video_packet(data, &header, now);
			return;
		}
		if header.payload_type != rtp::OPUS_PAYLOAD_TYPE || header.ssrc == self.transport.ssrc {
			return;
		}
		let Some(payload) = self
			.transport
			.cipher
			.open(data, header.header_len)
			.and_then(|plain| header.into_payload(plain))
		else {
			return;
		};
		let payload = if payload == SILENCE_FRAME {
			payload
		} else {
			let Some(decrypted) = self.transform.lock().decrypt(Codec::Opus, header.ssrc, &payload) else {
				return;
			};
			decrypted
		};
		let stream = match self.streams.entry(header.ssrc) {
			std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
			std::collections::hash_map::Entry::Vacant(entry) => match RemoteStream::new(now) {
				Ok(stream) => entry.insert(stream),
				Err(error) => {
					warn!(%error, "failed to create an opus decoder");
					return;
				}
			},
		};
		self.shared.counters.received.fetch_add(1, Ordering::Relaxed);
		if stream.push(header.sequence, header.timestamp, payload, now) == Push::Late {
			self.shared.counters.late.fetch_add(1, Ordering::Relaxed);
		}
	}

	fn on_rtcp(&mut self, data: &[u8]) {
		if data[1] != PSFB_PAYLOAD_TYPE || self.shared.video.ssrc() == 0 {
			return;
		}
		let format = data[0] & RTCP_FORMAT_MASK;
		if (format == PLI_FORMAT || format == FIR_FORMAT)
			&& self.transport.cipher.open(data, rtp::RTCP_HEADER_LEN).is_some()
		{
			self.shared.video.keyframe_wanted.store(true, Ordering::Relaxed);
		}
	}

	fn on_video_packet(&mut self, data: &[u8], header: &rtp::Header, now: Instant) {
		let ssrc = header.ssrc;
		if ssrc == self.shared.video.ssrc() {
			return;
		}
		let Some(stream) = self.shared.video.remote.lock().get(&ssrc).copied() else {
			return;
		};
		let Some(payload) = self
			.transport
			.cipher
			.open(data, header.header_len)
			.and_then(|plain| header.into_payload(plain))
		else {
			return;
		};
		let receiver = self.video_streams.entry(ssrc).or_insert_with(|| VideoReceiver {
			depacketizer: H264Depacketizer::default(),
			last_request: None,
			last_packet: now,
		});
		receiver.last_packet = now;
		let output = receiver
			.depacketizer
			.push(header.sequence, header.timestamp, header.marker, &payload);
		self.shared
			.counters
			.video_dropped
			.fetch_add(u64::from(output.dropped), Ordering::Relaxed);
		let mut wants_keyframe = output.lost || receiver.last_request.is_none();
		if let Some(frame) = output.frame {
			let clear = self.transform.lock().decrypt(Codec::H264, ssrc, &frame.data);
			match clear {
				Some(clear) => {
					self.shared
						.counters
						.video_received
						.fetch_add(1, Ordering::Relaxed);
					self.submit_frame(stream, clear, frame.timestamp);
				}
				None => {
					self.shared.counters.video_dropped.fetch_add(1, Ordering::Relaxed);
					if let Some(receiver) = self.video_streams.get_mut(&ssrc) {
						receiver.depacketizer.require_keyframe();
					}
					wants_keyframe = true;
				}
			}
		}
		if wants_keyframe {
			self.request_keyframe(ssrc, now);
		}
	}

	fn submit_frame(&mut self, stream: VideoStream, data: Vec<u8>, timestamp: u32) {
		if self.shared.video.sink.lock().is_none() {
			return;
		}
		if self.decoder.is_none() {
			self.decoder = DecodeWorker::spawn(self.shared.clone());
		}
		let Some(decoder) = &self.decoder else {
			return;
		};
		let accepted = decoder.submit(DecodeJob {
			stream,
			data,
			timestamp: Duration::from_secs_f64(f64::from(timestamp) / f64::from(video_rtp::CLOCK_HZ)),
		});
		if !accepted {
			self.shared.counters.video_dropped.fetch_add(1, Ordering::Relaxed);
		}
	}

	fn request_keyframe(&mut self, ssrc: u32, now: Instant) {
		let Some(receiver) = self.video_streams.get_mut(&ssrc) else {
			return;
		};
		if receiver
			.last_request
			.is_some_and(|last| now.duration_since(last) < KEYFRAME_REQUEST_INTERVAL)
		{
			return;
		}
		receiver.last_request = Some(now);
		let mut header = [0u8; rtp::RTCP_HEADER_LEN];
		header[0] = RTCP_VERSION_2 | PLI_FORMAT;
		header[1] = PSFB_PAYLOAD_TYPE;
		header[2..4].copy_from_slice(&PLI_LENGTH_WORDS.to_be_bytes());
		header[4..8].copy_from_slice(&self.transport.ssrc.to_be_bytes());
		if let Some(packet) = self.transport.cipher.seal(&header, &ssrc.to_be_bytes()) {
			let _ = self.transport.socket.try_send(&packet);
		}
	}

	async fn send_video_frames(&mut self) {
		while let Some(frame) = self.shared.video.pop() {
			let ssrc = self.shared.video.ssrc();
			if ssrc == 0 {
				continue;
			}
			let Some(sealed) = self.transform.lock().encrypt(Codec::H264, &frame.data) else {
				self.shared.counters.video_dropped.fetch_add(1, Ordering::Relaxed);
				continue;
			};
			for payload in video_rtp::payload_frame(&sealed) {
				let header = rtp::write_video_header(
					video_rtp::PAYLOAD_TYPE,
					payload.marker,
					self.video_sequence,
					frame.timestamp,
					ssrc,
				);
				self.video_sequence = self.video_sequence.wrapping_add(1);
				let mut body = Vec::with_capacity(rtp::VIDEO_EXTENSION_LEN + payload.data.len());
				body.extend_from_slice(&rtp::video_extension());
				body.extend_from_slice(&payload.data);
				if let Some(packet) = self.transport.cipher.seal(&header, &body) {
					let _ = self.transport.socket.send(&packet).await;
				}
			}
			self.shared.counters.video_sent.fetch_add(1, Ordering::Relaxed);
		}
	}

	fn playout_tick(&mut self, now: Instant) {
		if self.audio_io && self.playback.has_device() {
			for _ in 0..MAX_FRAMES_PER_TICK {
				if !self.playback.wants_frame() {
					break;
				}
				self.mix_frame();
				if let Err(error) = self.playback.push(self.mixer.output()) {
					warn!(%error, "playback resampling failed");
					break;
				}
			}
			return;
		}
		if self.next_playout + MAX_PLAYOUT_LAG < now {
			self.next_playout = now;
		}
		while self.next_playout <= now {
			let active = self.mix_frame();
			self.next_playout += FRAME_DURATION;
			#[cfg(feature = "testing")]
			if active {
				self.tap_output();
			}
			#[cfg(not(feature = "testing"))]
			let _ = active;
		}
	}

	#[cfg(feature = "testing")]
	fn tap_output(&self) {
		let frame = self
			.mixer
			.output()
			.iter()
			.map(|sample| (sample * f32::from(i16::MAX)) as i16)
			.collect();
		self.shared.test_output.lock().push(frame);
	}

	fn mix_frame(&mut self) -> bool {
		self.mixer.clear();
		let deafened = self.shared.deafened.load(Ordering::Relaxed);
		let users = self.shared.ssrc_users.lock();
		let volumes = self.shared.user_volumes.lock();
		let mut active = false;
		for (ssrc, stream) in &mut self.streams {
			let frame = stream.next_frame();
			if frame == Frame::Silence {
				continue;
			}
			active = true;
			if frame == Frame::Concealed {
				self.shared.counters.lost.fetch_add(1, Ordering::Relaxed);
			}
			if !deafened {
				let gain = users
					.get(ssrc)
					.and_then(|user| volumes.get(user))
					.copied()
					.unwrap_or(1.0);
				self.mixer.add(stream.pcm(), gain);
			}
		}
		drop((users, volumes));
		let master = if deafened {
			0.0
		} else {
			self.shared.output_volume.load()
		};
		let level = meter_level(rms(self.mixer.finish(master)));
		let previous = self.shared.output_level.load();
		self.shared.output_level.store(level.max(previous * METER_DECAY));
		active
	}

	fn video_housekeeping(&mut self, now: Instant) {
		let failed = std::mem::take(&mut *self.shared.video.decode_failures.lock());
		for ssrc in failed {
			if let Some(receiver) = self.video_streams.get_mut(&ssrc) {
				receiver.depacketizer.require_keyframe();
			}
		}
		let waiting: Vec<u32> = self
			.video_streams
			.iter()
			.filter(|(_, receiver)| receiver.depacketizer.waiting_for_keyframe())
			.map(|(ssrc, _)| *ssrc)
			.collect();
		for ssrc in waiting {
			self.request_keyframe(ssrc, now);
		}
		let remote = self.shared.video.remote.lock();
		self.video_streams.retain(|ssrc, receiver| {
			remote.contains_key(ssrc) && now.duration_since(receiver.last_packet) < STREAM_TTL
		});
	}

	fn housekeeping(&mut self, now: Instant) {
		self.video_housekeeping(now);
		self.streams.retain(|ssrc, stream| {
			let keep = stream.idle_since(now) < STREAM_TTL;
			if !keep {
				debug!(ssrc, "dropping idle voice stream");
			}
			keep
		});
		let jitter = self
			.streams
			.values()
			.map(RemoteStream::jitter_ms)
			.fold(0.0_f32, f32::max);
		self.shared.counters.jitter_ms.store(jitter);
	}
}
