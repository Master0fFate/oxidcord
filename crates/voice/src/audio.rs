use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
	Device, DeviceId, Error, ErrorKind, FromSample, Host, SampleFormat, SizedSample, Stream, StreamConfig,
};
use parking_lot::Mutex;
use tracing::warn;

use crate::error::{Result, VoiceError};

const CAPTURE_LIMIT_SECONDS: usize = 2;
const PLAYBACK_LIMIT_SECONDS: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
	pub id: String,
	pub name: String,
	pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioDevices {
	pub inputs: Vec<DeviceInfo>,
	pub outputs: Vec<DeviceInfo>,
}

pub fn devices() -> AudioDevices {
	let host = cpal::default_host();
	let default_input = host.default_input_device().and_then(|device| device.id().ok());
	let default_output = host.default_output_device().and_then(|device| device.id().ok());
	AudioDevices {
		inputs: host
			.input_devices()
			.map(|found| describe(found, default_input.as_ref()))
			.unwrap_or_default(),
		outputs: host
			.output_devices()
			.map(|found| describe(found, default_output.as_ref()))
			.unwrap_or_default(),
	}
}

fn describe(found: impl Iterator<Item = Device>, default: Option<&DeviceId>) -> Vec<DeviceInfo> {
	found
		.filter_map(|device| {
			let id = device.id().ok()?;
			let name = device.description().ok()?.name().to_owned();
			Some(DeviceInfo {
				is_default: default == Some(&id),
				id: id.to_string(),
				name,
			})
		})
		.collect()
}

pub(crate) struct Rings {
	capture: Mutex<VecDeque<f32>>,
	playback: Mutex<VecDeque<f32>>,
	system: Mutex<VecDeque<f32>>,
	capture_rate: AtomicU32,
	playback_rate: AtomicU32,
	system_rate: AtomicU32,
}

impl Rings {
	pub fn new() -> Self {
		Self {
			capture: Mutex::new(VecDeque::new()),
			playback: Mutex::new(VecDeque::new()),
			system: Mutex::new(VecDeque::new()),
			capture_rate: AtomicU32::new(0),
			playback_rate: AtomicU32::new(0),
			system_rate: AtomicU32::new(0),
		}
	}

	pub fn capture_rate(&self) -> u32 {
		self.capture_rate.load(Ordering::Acquire)
	}

	pub fn playback_rate(&self) -> u32 {
		self.playback_rate.load(Ordering::Acquire)
	}

	pub fn system_rate(&self) -> u32 {
		self.system_rate.load(Ordering::Acquire)
	}

	pub fn drain_capture(&self, into: &mut Vec<f32>) {
		into.extend(self.capture.lock().drain(..));
	}

	pub fn drain_system(&self, into: &mut Vec<f32>) {
		into.extend(self.system.lock().drain(..));
	}

	pub fn playback_len(&self) -> usize {
		self.playback.lock().len()
	}

	pub fn push_playback(&self, stereo: &[f32]) {
		let limit = self.playback_rate() as usize * 2 * PLAYBACK_LIMIT_SECONDS;
		let mut ring = self.playback.lock();
		ring.extend(stereo);
		let excess = ring.len().saturating_sub(limit);
		ring.drain(..excess);
	}

	pub fn clear_playback(&self) {
		self.playback.lock().clear();
	}

	fn push_capture(&self, mono: impl Iterator<Item = f32>) {
		let limit = self.capture_rate() as usize * CAPTURE_LIMIT_SECONDS;
		let mut ring = self.capture.lock();
		ring.extend(mono);
		let excess = ring.len().saturating_sub(limit);
		ring.drain(..excess);
	}

	#[cfg(windows)]
	fn push_system(&self, mono: impl Iterator<Item = f32>) {
		let limit = self.system_rate() as usize * CAPTURE_LIMIT_SECONDS;
		let mut ring = self.system.lock();
		ring.extend(mono);
		let excess = ring.len().saturating_sub(limit);
		ring.drain(..excess);
	}

	#[cfg(windows)]
	fn stop_system(&self) {
		self.system_rate.store(0, Ordering::Release);
		self.system.lock().clear();
	}
}

enum Command {
	Input(Option<String>),
	Output(Option<String>),
	#[cfg(windows)]
	SystemAudio(bool, mpsc::Sender<Result<()>>),
	Stop,
}

pub(crate) struct AudioEngine {
	commands: mpsc::Sender<Command>,
	thread: Option<JoinHandle<()>>,
}

impl AudioEngine {
	pub fn start(rings: Arc<Rings>, input: Option<String>, output: Option<String>) -> Result<Self> {
		let (commands, receiver) = mpsc::channel();
		let (ready_sender, ready) = mpsc::channel();
		let thread = thread::Builder::new()
			.name("voice-audio".to_owned())
			.spawn(move || run(&rings, input, output, &receiver, &ready_sender))
			.map_err(|error| VoiceError::Audio(error.to_string()))?;
		let _ = ready.recv();
		Ok(Self {
			commands,
			thread: Some(thread),
		})
	}

	pub fn set_input(&self, id: Option<String>) {
		let _ = self.commands.send(Command::Input(id));
	}

	pub fn set_output(&self, id: Option<String>) {
		let _ = self.commands.send(Command::Output(id));
	}

	#[cfg(windows)]
	pub fn set_system_audio(&self, enabled: bool) -> Result<()> {
		let (reply, result) = mpsc::channel();
		self.commands
			.send(Command::SystemAudio(enabled, reply))
			.map_err(|_| VoiceError::Audio("audio engine stopped".to_owned()))?;
		result
			.recv()
			.map_err(|_| VoiceError::Audio("audio engine stopped".to_owned()))?
	}
}

impl Drop for AudioEngine {
	fn drop(&mut self) {
		let _ = self.commands.send(Command::Stop);
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}
}

fn run(
	rings: &Arc<Rings>,
	input: Option<String>,
	output: Option<String>,
	commands: &mpsc::Receiver<Command>,
	ready: &mpsc::Sender<()>,
) {
	let host = cpal::default_host();
	let mut input_stream = open_input(&host, input.as_deref(), rings);
	let mut output_stream = open_output(&host, output.as_deref(), rings);
	#[cfg(windows)]
	let mut system_stream: Option<Stream> = None;
	let _ = ready.send(());
	while let Ok(command) = commands.recv() {
		match command {
			Command::Input(id) => {
				drop(input_stream.take());
				input_stream = open_input(&host, id.as_deref(), rings);
			}
			Command::Output(id) => {
				drop(output_stream.take());
				output_stream = open_output(&host, id.as_deref(), rings);
			}
			#[cfg(windows)]
			Command::SystemAudio(enabled, reply) => {
				drop(system_stream.take());
				rings.stop_system();
				let opened = if enabled {
					open_system(&host, rings).map(|stream| system_stream = Some(stream))
				} else {
					Ok(())
				};
				let _ = reply.send(opened);
			}
			Command::Stop => break,
		}
	}
	drop((input_stream, output_stream));
}

fn find_device(host: &Host, id: Option<&str>, input: bool) -> Option<Device> {
	let requested = id
		.and_then(|id| id.parse::<DeviceId>().ok())
		.and_then(|id| host.device_by_id(&id));
	if requested.is_none() && id.is_some() {
		warn!("requested audio device not found, using the default");
	}
	requested.or_else(|| {
		if input {
			host.default_input_device()
		} else {
			host.default_output_device()
		}
	})
}

fn open_input(host: &Host, id: Option<&str>, rings: &Arc<Rings>) -> Option<Stream> {
	rings.capture_rate.store(0, Ordering::Release);
	let device = find_device(host, id, true)?;
	match build_input(&device, rings) {
		Ok(stream) => Some(stream),
		Err(error) => {
			warn!(%error, "failed to open the input device");
			None
		}
	}
}

fn open_output(host: &Host, id: Option<&str>, rings: &Arc<Rings>) -> Option<Stream> {
	rings.playback_rate.store(0, Ordering::Release);
	rings.clear_playback();
	let device = find_device(host, id, false)?;
	match build_output(&device, rings) {
		Ok(stream) => Some(stream),
		Err(error) => {
			warn!(%error, "failed to open the output device");
			None
		}
	}
}

#[cfg(windows)]
fn open_system(host: &Host, rings: &Arc<Rings>) -> Result<Stream> {
	let device = host
		.default_output_device()
		.ok_or_else(|| VoiceError::Audio("no output device to capture".to_owned()))?;
	build_system(&device, rings).map_err(|error| VoiceError::Audio(error.to_string()))
}

fn unsupported_format(format: SampleFormat) -> Error {
	Error::with_message(ErrorKind::UnsupportedConfig, format!("sample format {format}"))
}

fn build_input(device: &Device, rings: &Arc<Rings>) -> std::result::Result<Stream, Error> {
	let supported = device.default_input_config()?;
	let config = supported.config();
	let stream = match supported.sample_format() {
		SampleFormat::F32 => input_stream::<f32>(device, config, rings),
		SampleFormat::I16 => input_stream::<i16>(device, config, rings),
		SampleFormat::U16 => input_stream::<u16>(device, config, rings),
		SampleFormat::I32 => input_stream::<i32>(device, config, rings),
		format => return Err(unsupported_format(format)),
	}?;
	rings.capture_rate.store(config.sample_rate, Ordering::Release);
	stream.play()?;
	Ok(stream)
}

#[cfg(windows)]
fn build_system(device: &Device, rings: &Arc<Rings>) -> std::result::Result<Stream, Error> {
	let supported = device.default_output_config()?;
	let config = supported.config();
	let stream = match supported.sample_format() {
		SampleFormat::F32 => system_stream::<f32>(device, config, rings),
		SampleFormat::I16 => system_stream::<i16>(device, config, rings),
		SampleFormat::U16 => system_stream::<u16>(device, config, rings),
		SampleFormat::I32 => system_stream::<i32>(device, config, rings),
		format => return Err(unsupported_format(format)),
	}?;
	rings.system_rate.store(config.sample_rate, Ordering::Release);
	stream.play()?;
	Ok(stream)
}

fn build_output(device: &Device, rings: &Arc<Rings>) -> std::result::Result<Stream, Error> {
	let supported = device.default_output_config()?;
	let config = supported.config();
	let stream = match supported.sample_format() {
		SampleFormat::F32 => output_stream::<f32>(device, config, rings),
		SampleFormat::I16 => output_stream::<i16>(device, config, rings),
		SampleFormat::U16 => output_stream::<u16>(device, config, rings),
		SampleFormat::I32 => output_stream::<i32>(device, config, rings),
		format => return Err(unsupported_format(format)),
	}?;
	rings.playback_rate.store(config.sample_rate, Ordering::Release);
	stream.play()?;
	Ok(stream)
}

fn input_stream<T>(
	device: &Device,
	config: StreamConfig,
	rings: &Arc<Rings>,
) -> std::result::Result<Stream, Error>
where
	T: SizedSample,
	f32: FromSample<T>,
{
	let channels = usize::from(config.channels);
	let rings = rings.clone();
	device.build_input_stream(
		config,
		move |data: &[T], _| {
			rings.push_capture(data.chunks_exact(channels).map(|frame| {
				frame.iter().map(|sample| sample.to_sample::<f32>()).sum::<f32>() / channels as f32
			}));
		},
		|error| warn!(%error, "input stream error"),
		None,
	)
}

#[cfg(windows)]
fn system_stream<T>(
	device: &Device,
	config: StreamConfig,
	rings: &Arc<Rings>,
) -> std::result::Result<Stream, Error>
where
	T: SizedSample,
	f32: FromSample<T>,
{
	let channels = usize::from(config.channels);
	let rings = rings.clone();
	device.build_input_stream(
		config,
		move |data: &[T], _| {
			rings.push_system(data.chunks_exact(channels).map(|frame| {
				frame.iter().map(|sample| sample.to_sample::<f32>()).sum::<f32>() / channels as f32
			}));
		},
		|error| warn!(%error, "system audio stream error"),
		None,
	)
}

fn output_stream<T>(
	device: &Device,
	config: StreamConfig,
	rings: &Arc<Rings>,
) -> std::result::Result<Stream, Error>
where
	T: SizedSample + FromSample<f32>,
{
	let channels = usize::from(config.channels);
	let rings = rings.clone();
	device.build_output_stream(
		config,
		move |data: &mut [T], _| {
			let mut ring = rings.playback.lock();
			for frame in data.chunks_exact_mut(channels) {
				let (Some(left), Some(right)) = (ring.pop_front(), ring.pop_front()) else {
					break;
				};
				if channels == 1 {
					frame[0] = T::from_sample((left + right) * 0.5);
				} else {
					frame[0] = T::from_sample(left);
					frame[1] = T::from_sample(right);
				}
			}
		},
		|error| warn!(%error, "output stream error"),
		None,
	)
}
