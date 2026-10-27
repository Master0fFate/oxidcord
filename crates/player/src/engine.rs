use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use media::MediaFetcher;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::video::well_known::{
	CODEC_ID_AV1, CODEC_ID_H264, CODEC_ID_HEVC, CODEC_ID_VP8, CODEC_ID_VP9,
};
use symphonia::core::errors::Error as FormatError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::packet::Packet;
use symphonia::core::units::{Time, TimeBase, Timestamp};
use symphonia::default::get_probe;
use tokio::runtime::Handle;
use voice::Resampler;

use crate::audio::AudioTrack;
use crate::error::{PlayerError, Result};
use crate::output::{OUTPUT_CHANNELS, Output};
use crate::source::HttpSource;
use crate::video::VideoTrack;
use crate::{Command, Frame, Options, Shared, State};

const AUDIO_AHEAD: Duration = Duration::from_millis(750);
const VIDEO_AHEAD: usize = 6;
const VIDEO_LIMIT: usize = 48;
const IDLE_WAIT: Duration = Duration::from_millis(250);
const MAX_WAIT: Duration = Duration::from_millis(15);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
const LATE_FRAME: Duration = Duration::from_millis(60);
const PREROLL_PACKETS: usize = 600;
const SEEK_BACK_START: Duration = Duration::from_secs(1);
const SEEK_BACK_LIMIT: Duration = Duration::from_secs(64);
const SCAN_PACKETS: usize = 256;
const MAX_BAD_PACKETS: usize = 64;

pub(crate) fn run(
	shared: Arc<Shared>,
	commands: Receiver<Command>,
	fetcher: MediaFetcher,
	runtime: Handle,
	url: String,
	options: Options,
) {
	let outcome =
		Engine::open(&shared, commands, fetcher, runtime, &url, options).and_then(|mut engine| engine.run());
	if let Err(error) = outcome
		&& !shared.closed()
	{
		tracing::debug!(url = media::redact_url(&url), %error, "media playback failed");
		shared.update(|status| {
			status.state = State::Failed;
			status.buffering = false;
		});
	}
}

fn to_duration(time: Time) -> Duration {
	Duration::from_nanos(u64::try_from(time.as_nanos()).unwrap_or(0))
}

fn packet_time(time_base: Option<TimeBase>, ts: Timestamp) -> Duration {
	time_base
		.and_then(|base| base.calc_time(ts))
		.map_or(Duration::ZERO, to_duration)
}

fn track_duration(track: &Track) -> Option<Duration> {
	let base = track.time_base?;
	base.calc_duration(track.duration?).map(to_duration)
}

fn media_duration(format: &dyn FormatReader) -> Option<Duration> {
	let info = format.media_info();
	let time = info.time_base?.calc_duration(info.duration?)?;
	Some(to_duration(time)).filter(|duration| !duration.is_zero())
}

fn extension(url: &str) -> Option<&str> {
	let path = url.split(['?', '#']).next()?;
	let file = path.rsplit('/').next()?;
	file.rsplit_once('.').map(|(_, extension)| extension)
}

fn open_format(source: HttpSource, hint: &Hint) -> Result<Box<dyn FormatReader>> {
	let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
	Ok(get_probe().probe(hint, stream, FormatOptions::default(), MetadataOptions::default())?)
}

struct AudioLane {
	id: u32,
	time_base: Option<TimeBase>,
	track: AudioTrack,
	resampler: Resampler,
	output: Output,
	resampled: Vec<f32>,
}

impl AudioLane {
	fn open(track: &Track, shared: &Shared) -> Result<Self> {
		let params = track
			.codec_params
			.as_ref()
			.ok_or_else(|| PlayerError::Unsupported("audio track without parameters".to_owned()))?;
		let decoder = AudioTrack::open(params)?;
		let output = Output::open(shared.volume.clone())?;
		let resampler = Resampler::new(decoder.rate(), output.rate(), OUTPUT_CHANNELS)?;
		Ok(Self {
			id: track.id,
			time_base: track.time_base,
			track: decoder,
			resampler,
			output,
			resampled: Vec::new(),
		})
	}

	fn buffered(&self) -> Duration {
		Duration::from_secs_f64(self.output.buffered_frames() as f64 / f64::from(self.output.rate()))
	}

	fn played(&self) -> Duration {
		self.output.played()
	}

	fn reset(&mut self, from_start: bool) -> Result<()> {
		self.track.reset(from_start);
		self.output.reset();
		self.resampler = Resampler::new(self.track.rate(), self.output.rate(), OUTPUT_CHANNELS)?;
		Ok(())
	}

	fn push(&mut self, packet: &Packet, skip_before: Option<Duration>) -> Result<()> {
		let start = packet_time(self.time_base, packet.pts);
		let rate = self.track.rate();
		let decoded = match self.track.decode(packet) {
			Ok(decoded) => decoded,
			Err(PlayerError::Format(error)) => {
				tracing::trace!(%error, "skipping an undecodable audio packet");
				return Ok(());
			}
			Err(error) => return Err(error),
		};
		let skip_frames = skip_before.filter(|target| *target > start).map_or(0, |target| {
			((target - start).as_secs_f64() * f64::from(rate)) as usize
		});
		let skip_samples = (skip_frames * OUTPUT_CHANNELS).min(decoded.len());
		self.resampled.clear();
		self.resampler
			.process(&decoded[skip_samples..], &mut self.resampled)?;
		self.output.push(&self.resampled);
		Ok(())
	}
}

struct VideoLane {
	id: u32,
	time_base: Option<TimeBase>,
	track: VideoTrack,
}

enum Clock {
	Audio {
		base: Duration,
	},
	Wall {
		base: Duration,
		started: Option<Instant>,
	},
}

struct Engine<'a> {
	shared: &'a Shared,
	commands: Receiver<Command>,
	source: HttpSource,
	hint: Hint,
	format: Box<dyn FormatReader>,
	audio: Option<AudioLane>,
	video: Option<VideoLane>,
	clock: Clock,
	queued: VecDeque<Packet>,
	frames: VecDeque<Frame>,
	eof: bool,
	state: State,
	looping: bool,
	autoplay: bool,
	max_size: (u32, u32),
	skip_before: Option<Duration>,
	duration: Option<Duration>,
	last_progress: Instant,
}

fn is_known_video(params: &CodecParameters) -> bool {
	matches!(
		params,
		CodecParameters::Video(video)
			if [CODEC_ID_H264, CODEC_ID_HEVC, CODEC_ID_VP8, CODEC_ID_VP9, CODEC_ID_AV1].contains(&video.codec)
	)
}

impl<'a> Engine<'a> {
	fn open(
		shared: &'a Shared,
		commands: Receiver<Command>,
		fetcher: MediaFetcher,
		runtime: Handle,
		url: &str,
		options: Options,
	) -> Result<Self> {
		let source = HttpSource::open(fetcher, runtime, url.to_owned(), shared.closed.clone())
			.map_err(|error| PlayerError::Source(error.to_string()))?;
		let mut hint = Hint::new();
		if let Some(extension) = extension(url) {
			hint.with_extension(extension);
		}
		let format = open_format(source.reopen(), &hint)?;
		let video = match format
			.default_track(TrackType::Video)
			.filter(|track| track.codec_params.as_ref().is_some_and(is_known_video))
		{
			Some(track) => Some(VideoLane {
				id: track.id,
				time_base: track.time_base,
				track: VideoTrack::open(track.codec_params.as_ref().expect("checked above"))?,
			}),
			None => None,
		};
		let audio =
			format
				.default_track(TrackType::Audio)
				.and_then(|track| match AudioLane::open(track, shared) {
					Ok(lane) => Some(lane),
					Err(error) => {
						tracing::debug!(%error, "media audio unavailable");
						None
					}
				});
		if audio.is_none() && video.is_none() {
			return Err(PlayerError::Unsupported("no playable track".to_owned()));
		}
		let duration = [
			video.as_ref().map(|lane| lane.id),
			audio.as_ref().map(|lane| lane.id),
		]
		.into_iter()
		.flatten()
		.filter_map(|id| format.tracks().iter().find(|track| track.id == id))
		.filter_map(track_duration)
		.max()
		.or_else(|| media_duration(format.as_ref()));
		let clock = if audio.is_some() {
			Clock::Audio { base: Duration::ZERO }
		} else {
			Clock::Wall {
				base: Duration::ZERO,
				started: None,
			}
		};
		let size = video.as_ref().and_then(|lane| lane.track.size());
		let has_audio = audio.is_some();
		shared.update(|status| {
			status.state = State::Paused;
			status.duration = duration;
			status.video = size;
			status.audio = has_audio;
			status.buffering = false;
		});
		Ok(Self {
			shared,
			commands,
			source,
			hint,
			format,
			audio,
			video,
			clock,
			queued: VecDeque::new(),
			frames: VecDeque::new(),
			eof: false,
			state: State::Paused,
			looping: options.looping,
			autoplay: options.autoplay,
			max_size: options.max_size,
			skip_before: None,
			duration,
			last_progress: Instant::now(),
		})
	}

	fn run(&mut self) -> Result<()> {
		self.preroll()?;
		if self.autoplay {
			self.resume();
		}
		loop {
			if !self.drain_commands()? {
				return Ok(());
			}
			if self.state != State::Playing {
				match self.commands.recv_timeout(IDLE_WAIT) {
					Ok(command) => self.apply(command)?,
					Err(RecvTimeoutError::Timeout) => {}
					Err(RecvTimeoutError::Disconnected) => return Ok(()),
				}
				continue;
			}
			let now = self.position();
			self.present(now);
			self.report(now);
			if self.finished() {
				if self.looping {
					self.seek(Duration::ZERO)?;
					self.resume();
				} else {
					self.end();
				}
				continue;
			}
			if self.wants_data() {
				self.pump(now)?;
				continue;
			}
			let wait = self
				.frames
				.front()
				.map_or(MAX_WAIT, |frame| frame.pts.saturating_sub(now))
				.min(MAX_WAIT);
			match self.commands.recv_timeout(wait) {
				Ok(command) => self.apply(command)?,
				Err(RecvTimeoutError::Timeout) => {}
				Err(RecvTimeoutError::Disconnected) => return Ok(()),
			}
		}
	}

	fn drain_commands(&mut self) -> Result<bool> {
		if self.shared.closed() {
			return Ok(false);
		}
		loop {
			match self.commands.try_recv() {
				Ok(command) => self.apply(command)?,
				Err(TryRecvError::Empty) => return Ok(true),
				Err(TryRecvError::Disconnected) => return Ok(false),
			}
		}
	}

	fn apply(&mut self, command: Command) -> Result<()> {
		match command {
			Command::Play => {
				if self.state == State::Ended {
					self.seek(Duration::ZERO)?;
				}
				self.resume();
			}
			Command::Pause => self.pause(),
			Command::Seek(target) => {
				let target = self.duration.map_or(target, |duration| target.min(duration));
				self.seek(target)?;
				if self.state == State::Ended {
					self.state = State::Paused;
					self.shared.update(|status| status.state = State::Paused);
				}
				if self.state != State::Playing {
					self.preroll()?;
				}
			}
			Command::Resize(max_size) => self.max_size = max_size,
		}
		Ok(())
	}

	fn position(&self) -> Duration {
		match (&self.clock, &self.audio) {
			(Clock::Audio { base }, Some(lane)) => *base + lane.played(),
			(Clock::Wall { base, started }, _) => *base + started.map_or(Duration::ZERO, |at| at.elapsed()),
			(Clock::Audio { base }, None) => *base,
		}
	}

	fn set_clock(&mut self, at: Duration) {
		self.clock = match self.clock {
			Clock::Audio { .. } => Clock::Audio { base: at },
			Clock::Wall { .. } => Clock::Wall {
				base: at,
				started: (self.state == State::Playing).then(Instant::now),
			},
		};
	}

	fn resume(&mut self) {
		if self.state == State::Playing {
			return;
		}
		self.state = State::Playing;
		if let Clock::Wall { started, .. } = &mut self.clock {
			*started = Some(Instant::now());
		}
		if let Some(lane) = &self.audio {
			lane.output.resume();
		}
		self.shared.update(|status| status.state = State::Playing);
	}

	fn pause(&mut self) {
		if self.state != State::Playing {
			return;
		}
		let now = self.position();
		self.state = State::Paused;
		if let Clock::Wall { base, started } = &mut self.clock {
			*base = now;
			*started = None;
		}
		if let Some(lane) = &self.audio {
			lane.output.pause();
		}
		self.shared.update(|status| {
			status.state = State::Paused;
			status.position = now;
		});
	}

	fn end(&mut self) {
		let at = self.duration.unwrap_or_else(|| self.position());
		self.duration = Some(at);
		self.state = State::Ended;
		if let Some(lane) = &self.audio {
			lane.output.pause();
		}
		self.shared.update(|status| {
			status.state = State::Ended;
			status.position = at;
			status.duration = Some(at);
		});
	}

	fn present(&mut self, now: Duration) {
		let mut latest = None;
		while self.frames.front().is_some_and(|frame| frame.pts <= now) {
			latest = self.frames.pop_front();
		}
		if let Some(frame) = latest {
			self.shared.publish(frame);
		}
	}

	fn report(&mut self, now: Duration) {
		if self.last_progress.elapsed() < PROGRESS_INTERVAL {
			return;
		}
		self.last_progress = Instant::now();
		let position = self.duration.map_or(now, |duration| now.min(duration));
		self.shared.update(|status| status.position = position);
	}

	fn finished(&self) -> bool {
		self.eof
			&& self.queued.is_empty()
			&& self.frames.is_empty()
			&& self
				.audio
				.as_ref()
				.is_none_or(|lane| lane.output.buffered_frames() == 0)
	}

	fn wants_data(&self) -> bool {
		if self.eof && self.queued.is_empty() {
			return false;
		}
		let audio_needs = self
			.audio
			.as_ref()
			.is_some_and(|lane| lane.buffered() < AUDIO_AHEAD);
		let video_needs = self.video.is_some() && self.frames.len() < VIDEO_AHEAD;
		(audio_needs || video_needs) && self.frames.len() < VIDEO_LIMIT
	}

	fn next_packet(&mut self) -> Result<Option<Packet>> {
		match self.queued.pop_front() {
			Some(packet) => Ok(Some(packet)),
			None => self.read(),
		}
	}

	fn read(&mut self) -> Result<Option<Packet>> {
		let mut bad = 0usize;
		loop {
			match self.format.next_packet() {
				Ok(packet) => return Ok(packet),
				Err(FormatError::ResetRequired) => {
					if let Some(lane) = &mut self.video {
						lane.track.reset()?;
					}
				}
				Err(FormatError::DecodeError(error)) if bad < MAX_BAD_PACKETS => {
					bad += 1;
					tracing::trace!(error, "skipping a malformed media packet");
				}
				Err(FormatError::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
					return Ok(None);
				}
				Err(error) => return Err(error.into()),
			}
		}
	}

	fn pump(&mut self, now: Duration) -> Result<()> {
		let Some(packet) = self.next_packet()? else {
			self.finish_stream()?;
			return Ok(());
		};
		self.decode(&packet, Some(now))
	}

	fn decode(&mut self, packet: &Packet, now: Option<Duration>) -> Result<()> {
		let skip_before = self.skip_before;
		if let Some(lane) = self.audio.as_mut().filter(|lane| lane.id == packet.track_id) {
			return lane.push(packet, skip_before);
		}
		let Some(lane) = self.video.as_mut().filter(|lane| lane.id == packet.track_id) else {
			return Ok(());
		};
		let pts = packet_time(lane.time_base, packet.pts);
		let frames = &mut self.frames;
		let max_size = self.max_size;
		let decoded = lane.track.decode(&packet.data, pts, &mut |picture| {
			let cutoff = skip_before.max(now).unwrap_or_default();
			if picture.pts + LATE_FRAME < cutoff {
				return;
			}
			frames.push_back(picture.to_frame(max_size));
		});
		if let Err(error) = decoded {
			tracing::trace!(%error, "skipping an undecodable video packet");
		}
		Ok(())
	}

	fn finish_stream(&mut self) -> Result<()> {
		self.eof = true;
		if let Some(lane) = &mut self.video {
			let frames = &mut self.frames;
			let max_size = self.max_size;
			let finished = lane
				.track
				.finish(&mut |picture| frames.push_back(picture.to_frame(max_size)));
			if let Err(error) = finished {
				tracing::trace!(%error, "could not drain the video decoder");
			}
		}
		Ok(())
	}

	fn seek(&mut self, target: Duration) -> Result<()> {
		self.frames.clear();
		self.queued.clear();
		if std::mem::take(&mut self.eof) {
			self.format = open_format(self.source.reopen(), &self.hint)?;
		}
		let from_start = target.is_zero();
		if let Some(lane) = &mut self.audio {
			lane.reset(from_start)?;
		}
		if let Some(lane) = &mut self.video {
			lane.track.reset()?;
		}
		match &self.video {
			Some(lane) => {
				let id = lane.id;
				self.seek_keyframe(id, target)?;
			}
			None => {
				let id = self.audio.as_ref().map(|lane| lane.id);
				self.seek_to(target, id)?;
			}
		}
		self.skip_before = (!from_start).then_some(target);
		self.set_clock(target);
		self.last_progress = Instant::now();
		self.shared.update(|status| status.position = target);
		Ok(())
	}

	fn seek_to(&mut self, target: Duration, track_id: Option<u32>) -> Result<()> {
		let time = Time::try_from_nanos_u128(target.as_nanos()).unwrap_or(Time::ZERO);
		match self
			.format
			.seek(SeekMode::Accurate, SeekTo::Time { time, track_id })
		{
			Ok(_) => Ok(()),
			Err(FormatError::SeekError(_)) if target.is_zero() => Ok(()),
			Err(error) => Err(error.into()),
		}
	}

	fn seek_keyframe(&mut self, video_id: u32, target: Duration) -> Result<()> {
		let mut back = Duration::ZERO;
		loop {
			let at = target.saturating_sub(back);
			self.seek_to(at, Some(video_id))?;
			let mut scanned = Vec::new();
			let keyframe = loop {
				let Some(packet) = self.read()? else {
					break true;
				};
				let found = (packet.track_id == video_id).then(|| {
					self.video
						.as_ref()
						.is_some_and(|lane| lane.track.is_keyframe(&packet.data))
				});
				scanned.push(packet);
				if let Some(found) = found {
					break found;
				}
				if scanned.len() >= SCAN_PACKETS {
					break true;
				}
			};
			if keyframe || at.is_zero() || back >= SEEK_BACK_LIMIT {
				let first_video = scanned.iter().position(|packet| packet.track_id == video_id);
				let keep_from = match (keyframe, first_video) {
					(false, Some(index)) => scanned
						.iter()
						.skip(index + 1)
						.position(|packet| packet.track_id == video_id)
						.map_or(index, |offset| index + 1 + offset),
					_ => 0,
				};
				self.queued = scanned.into_iter().skip(keep_from).collect();
				return Ok(());
			}
			back = if back.is_zero() { SEEK_BACK_START } else { back * 2 };
		}
	}

	fn preroll(&mut self) -> Result<()> {
		if self.video.is_none() {
			return Ok(());
		}
		let mut held = Vec::new();
		for _ in 0..PREROLL_PACKETS {
			if !self.frames.is_empty() {
				break;
			}
			let Some(packet) = self.next_packet()? else {
				self.finish_stream()?;
				break;
			};
			if self.audio.as_ref().is_some_and(|lane| lane.id == packet.track_id) {
				held.push(packet);
			} else {
				self.decode(&packet, None)?;
			}
		}
		held.extend(self.queued.drain(..));
		self.queued = held.into();
		let Some(first) = self.frames.front() else {
			return Ok(());
		};
		let shown = Frame {
			width: first.width,
			height: first.height,
			bgra: first.bgra.clone(),
			pts: first.pts,
		};
		self.shared.publish(shown);
		Ok(())
	}
}
