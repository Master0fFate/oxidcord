use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::time::Duration;

use libloading::Library;

use super::convert::{Chroma, Picture};
use crate::error::{PlayerError, Result};

const PADDING: usize = 64;
const MAX_THREADS: usize = 4;
const SAMPLE_FORMAT_U8: c_int = 0;
const SAMPLE_FORMAT_S16: c_int = 1;
const SAMPLE_FORMAT_S32: c_int = 2;
const SAMPLE_FORMAT_FLT: c_int = 3;
const SAMPLE_FORMAT_DBL: c_int = 4;
const SAMPLE_FORMAT_U8P: c_int = 5;
const SAMPLE_FORMAT_S16P: c_int = 6;
const SAMPLE_FORMAT_S32P: c_int = 7;
const SAMPLE_FORMAT_FLTP: c_int = 8;
const SAMPLE_FORMAT_DBLP: c_int = 9;
const MAX_PLANES: usize = 8;
const NEW_EXTRADATA: c_int = 1;
#[cfg(target_os = "macos")]
const EAGAIN: c_int = 35;
#[cfg(not(target_os = "macos"))]
const EAGAIN: c_int = 11;
const AVERROR_EOF: c_int = -0x2046_4F45;
const NO_PTS: i64 = i64::MIN;
const CODECS_DIR: &str = "codecs";
const VERSIONS: [(u32, u32, u32); 5] = [(62, 60, 6), (61, 59, 5), (60, 58, 4), (59, 57, 3), (58, 56, 3)];
#[cfg(target_os = "macos")]
const SYSTEM_DIRS: [&str; 5] = [
	"/opt/homebrew/lib",
	"/usr/local/lib",
	"/opt/local/lib",
	"/Library/Frameworks",
	"/usr/lib",
];
#[cfg(target_os = "linux")]
const SYSTEM_DIRS: [&str; 6] = [
	"/usr/lib/x86_64-linux-gnu",
	"/usr/lib/aarch64-linux-gnu",
	"/usr/lib64",
	"/usr/lib",
	"/usr/local/lib",
	"/lib/x86_64-linux-gnu",
];

#[repr(C)]
struct PacketHead {
	buf: *mut c_void,
	pts: i64,
	dts: i64,
	data: *mut u8,
	size: c_int,
}

#[repr(C)]
struct FrameHead {
	data: [*mut u8; 8],
	linesize: [c_int; 8],
	extended_data: *mut *mut u8,
	width: c_int,
	height: c_int,
	nb_samples: c_int,
	format: c_int,
	_picture: [c_int; 4],
	pts: i64,
}

struct Api {
	find_decoder: unsafe extern "C" fn(*const c_char) -> *const c_void,
	alloc_context: unsafe extern "C" fn(*const c_void) -> *mut c_void,
	open: unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> c_int,
	send_packet: unsafe extern "C" fn(*mut c_void, *const PacketHead) -> c_int,
	receive_frame: unsafe extern "C" fn(*mut c_void, *mut FrameHead) -> c_int,
	flush: unsafe extern "C" fn(*mut c_void),
	free_context: unsafe extern "C" fn(*mut *mut c_void),
	packet_alloc: unsafe extern "C" fn() -> *mut PacketHead,
	packet_free: unsafe extern "C" fn(*mut *mut PacketHead),
	frame_alloc: unsafe extern "C" fn() -> *mut FrameHead,
	frame_free: unsafe extern "C" fn(*mut *mut FrameHead),
	frame_unref: unsafe extern "C" fn(*mut FrameHead),
	set_int: unsafe extern "C" fn(*mut c_void, *const c_char, i64, c_int) -> c_int,
	packet_side_data: unsafe extern "C" fn(*mut PacketHead, c_int, usize) -> *mut u8,
	packet_free_side_data: unsafe extern "C" fn(*mut PacketHead),
	pixel_format_name: unsafe extern "C" fn(c_int) -> *const c_char,
	_libraries: Vec<Library>,
}

fn file_names((codec, util, resample): (u32, u32, u32)) -> [String; 3] {
	if cfg!(windows) {
		[
			format!("avutil-{util}.dll"),
			format!("swresample-{resample}.dll"),
			format!("avcodec-{codec}.dll"),
		]
	} else if cfg!(target_os = "macos") {
		[
			format!("libavutil.{util}.dylib"),
			format!("libswresample.{resample}.dylib"),
			format!("libavcodec.{codec}.dylib"),
		]
	} else {
		[
			format!("libavutil.so.{util}"),
			format!("libswresample.so.{resample}"),
			format!("libavcodec.so.{codec}"),
		]
	}
}

fn search_dirs() -> Vec<PathBuf> {
	let mut dirs = Vec::new();
	if let Some(exe_dir) = std::env::current_exe()
		.ok()
		.and_then(|exe| exe.parent().map(Path::to_path_buf))
	{
		dirs.push(exe_dir.join(CODECS_DIR));
		dirs.push(exe_dir);
	}
	#[cfg(any(target_os = "macos", target_os = "linux"))]
	dirs.extend(SYSTEM_DIRS.iter().map(PathBuf::from));
	dirs
}

fn open_set(names: &[String; 3], dir: Option<&Path>) -> Option<Vec<Library>> {
	let mut libraries = Vec::with_capacity(names.len());
	for (index, name) in names.iter().enumerate() {
		let target = dir.map_or_else(|| PathBuf::from(name), |dir| dir.join(name));
		if dir.is_some() && !target.is_file() {
			if index == 1 {
				continue;
			}
			return None;
		}
		match unsafe { Library::new(&target) } {
			Ok(library) => libraries.push(library),
			Err(_) if index == 1 => {}
			Err(error) => {
				tracing::debug!(%error, path = %target.display(), "codec pack library did not load");
				return None;
			}
		}
	}
	Some(libraries)
}

fn load_libraries() -> Option<Vec<Library>> {
	let dirs = search_dirs();
	for version in VERSIONS {
		let names = file_names(version);
		for dir in &dirs {
			if let Some(libraries) = open_set(&names, Some(dir)) {
				return Some(libraries);
			}
		}
		if !cfg!(windows)
			&& let Some(libraries) = open_set(&names, None)
		{
			return Some(libraries);
		}
	}
	None
}

macro_rules! symbol {
	($library:expr, $name:literal) => {
		*unsafe { $library.get(concat!($name, "\0").as_bytes()) }.ok()?
	};
}

impl Api {
	fn load() -> Option<Self> {
		let libraries = load_libraries()?;
		let util = libraries.first()?;
		let codec = libraries.last()?;
		let api = Self {
			find_decoder: symbol!(codec, "avcodec_find_decoder_by_name"),
			alloc_context: symbol!(codec, "avcodec_alloc_context3"),
			open: symbol!(codec, "avcodec_open2"),
			send_packet: symbol!(codec, "avcodec_send_packet"),
			receive_frame: symbol!(codec, "avcodec_receive_frame"),
			flush: symbol!(codec, "avcodec_flush_buffers"),
			free_context: symbol!(codec, "avcodec_free_context"),
			packet_alloc: symbol!(codec, "av_packet_alloc"),
			packet_free: symbol!(codec, "av_packet_free"),
			frame_alloc: symbol!(util, "av_frame_alloc"),
			frame_free: symbol!(util, "av_frame_free"),
			frame_unref: symbol!(util, "av_frame_unref"),
			set_int: symbol!(util, "av_opt_set_int"),
			packet_side_data: symbol!(codec, "av_packet_new_side_data"),
			packet_free_side_data: symbol!(codec, "av_packet_free_side_data"),
			pixel_format_name: symbol!(util, "av_get_pix_fmt_name"),
			_libraries: libraries,
		};
		Some(api)
	}
}

pub fn available() -> bool {
	api().is_some()
}

fn api() -> Option<&'static Api> {
	static API: OnceLock<Option<Api>> = OnceLock::new();
	API.get_or_init(|| {
		let loaded = Api::load();
		if loaded.is_none() {
			tracing::debug!("no ffmpeg codec pack found, using platform decoders");
		}
		loaded
	})
	.as_ref()
}

#[derive(Clone, Copy)]
enum Layout {
	Planar { wide: bool },
	Interleaved,
}

fn layout(name: &[u8]) -> Option<Layout> {
	match name {
		b"yuv420p" | b"yuvj420p" => Some(Layout::Planar { wide: false }),
		b"yuv420p10le" => Some(Layout::Planar { wide: true }),
		b"nv12" => Some(Layout::Interleaved),
		_ => None,
	}
}

pub struct FfDecoder {
	api: &'static Api,
	context: *mut c_void,
	packet: *mut PacketHead,
	frame: *mut FrameHead,
	scratch: Vec<u8>,
}

unsafe impl Send for FfDecoder {}

impl FfDecoder {
	pub fn open(names: &[&CStr]) -> Result<Self> {
		let api = api().ok_or_else(|| PlayerError::Unsupported("codec pack unavailable".to_owned()))?;
		let codec = names
			.iter()
			.map(|name| unsafe { (api.find_decoder)(name.as_ptr()) })
			.find(|codec| !codec.is_null())
			.ok_or_else(|| PlayerError::Unsupported("codec pack has no matching decoder".to_owned()))?;
		let decoder = Self {
			api,
			context: unsafe { (api.alloc_context)(codec) },
			packet: unsafe { (api.packet_alloc)() },
			frame: unsafe { (api.frame_alloc)() },
			scratch: Vec::new(),
		};
		if decoder.context.is_null() || decoder.packet.is_null() || decoder.frame.is_null() {
			return Err(PlayerError::Video("codec pack allocation failed".to_owned()));
		}
		let threads = std::thread::available_parallelism().map_or(1, |count| count.get().min(MAX_THREADS));
		unsafe { (api.set_int)(decoder.context, c"threads".as_ptr(), threads as i64, 0) };
		let opened = unsafe { (api.open)(decoder.context, codec, null_mut()) };
		if opened < 0 {
			return Err(PlayerError::Video(format!(
				"codec pack could not open a decoder ({opened})"
			)));
		}
		Ok(decoder)
	}

	pub fn decode(&mut self, data: &[u8], pts: Duration, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		self.scratch.clear();
		self.scratch.reserve(data.len() + PADDING);
		self.scratch.extend_from_slice(data);
		self.scratch.resize(data.len() + PADDING, 0);
		loop {
			let sent = unsafe {
				(*self.packet).data = self.scratch.as_mut_ptr();
				(*self.packet).size = data.len() as c_int;
				(*self.packet).pts = i64::try_from(pts.as_micros()).unwrap_or(NO_PTS);
				let sent = (self.api.send_packet)(self.context, self.packet);
				(*self.packet).data = null_mut();
				(*self.packet).size = 0;
				(*self.packet).pts = NO_PTS;
				sent
			};
			self.receive(sink)?;
			match sent {
				code if code == -EAGAIN => continue,
				code if code < 0 => {
					return Err(PlayerError::Video(format!(
						"codec pack rejected a packet ({code})"
					)));
				}
				_ => return Ok(()),
			}
		}
	}

	pub fn finish(&mut self, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		unsafe { (self.api.send_packet)(self.context, std::ptr::null()) };
		self.receive(sink)
	}

	pub fn reset(&mut self) {
		unsafe { (self.api.flush)(self.context) };
	}

	fn receive(&mut self, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		loop {
			let received = unsafe { (self.api.receive_frame)(self.context, self.frame) };
			if received == -EAGAIN || received == AVERROR_EOF {
				return Ok(());
			}
			if received < 0 {
				return Err(PlayerError::Video(format!(
					"codec pack decode failed ({received})"
				)));
			}
			self.emit(sink);
			unsafe { (self.api.frame_unref)(self.frame) };
		}
	}

	fn emit(&self, sink: &mut dyn FnMut(Picture)) {
		let frame = unsafe { &*self.frame };
		let name = unsafe { (self.api.pixel_format_name)(frame.format) };
		if name.is_null() {
			return;
		}
		let Some(layout) = layout(unsafe { CStr::from_ptr(name) }.to_bytes()) else {
			return;
		};
		let (width, height) = (
			frame.width.max(0) as usize & !1,
			frame.height.max(0) as usize & !1,
		);
		let planes = match layout {
			Layout::Planar { .. } => 3,
			Layout::Interleaved => 2,
		};
		if width == 0 || height == 0 || frame.linesize[..planes].iter().any(|stride| *stride <= 0) {
			return;
		}
		let strides = frame.linesize.map(|stride| stride.max(0) as usize);
		let plane = |index: usize, rows: usize| unsafe {
			std::slice::from_raw_parts(frame.data[index].cast_const(), strides[index] * rows)
		};
		let (chroma, wide) = match layout {
			Layout::Planar { wide } => (
				Chroma::Planar {
					u: plane(1, height / 2),
					v: plane(2, height / 2),
					stride: strides[1],
				},
				wide,
			),
			Layout::Interleaved => (
				Chroma::Interleaved {
					uv: plane(1, height / 2),
					stride: strides[1],
				},
				false,
			),
		};
		sink(Picture {
			width,
			height,
			luma: plane(0, height),
			luma_stride: strides[0],
			chroma,
			wide,
			pts: Duration::from_micros(u64::try_from(frame.pts).unwrap_or(0)),
		});
	}
}

impl Drop for FfDecoder {
	fn drop(&mut self) {
		unsafe {
			(self.api.frame_free)(&mut self.frame);
			(self.api.packet_free)(&mut self.packet);
			(self.api.free_context)(&mut self.context);
		}
	}
}

fn bytes_per_sample(format: c_int) -> usize {
	match format {
		SAMPLE_FORMAT_U8 | SAMPLE_FORMAT_U8P => 1,
		SAMPLE_FORMAT_S16 | SAMPLE_FORMAT_S16P => 2,
		SAMPLE_FORMAT_S32 | SAMPLE_FORMAT_S32P | SAMPLE_FORMAT_FLT | SAMPLE_FORMAT_FLTP => 4,
		SAMPLE_FORMAT_DBL | SAMPLE_FORMAT_DBLP => 8,
		_ => 0,
	}
}

fn is_planar(format: c_int) -> bool {
	matches!(
		format,
		SAMPLE_FORMAT_U8P | SAMPLE_FORMAT_S16P | SAMPLE_FORMAT_S32P | SAMPLE_FORMAT_FLTP | SAMPLE_FORMAT_DBLP
	)
}

fn sample_at(data: &[u8], index: usize, format: c_int) -> f32 {
	let bytes = bytes_per_sample(format);
	let Some(raw) = data.get(index * bytes..index * bytes + bytes) else {
		return 0.;
	};
	match format {
		SAMPLE_FORMAT_U8 | SAMPLE_FORMAT_U8P => (f32::from(raw[0]) - 128.) / 128.,
		SAMPLE_FORMAT_S16 | SAMPLE_FORMAT_S16P => f32::from(i16::from_le_bytes([raw[0], raw[1]])) / 32768.,
		SAMPLE_FORMAT_S32 | SAMPLE_FORMAT_S32P => {
			i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f32 / 2147483648.
		}
		SAMPLE_FORMAT_FLT | SAMPLE_FORMAT_FLTP => f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
		SAMPLE_FORMAT_DBL | SAMPLE_FORMAT_DBLP => {
			let mut wide = [0u8; 8];
			wide.copy_from_slice(raw);
			f64::from_le_bytes(wide) as f32
		}
		_ => 0.,
	}
}

pub struct FfAudioDecoder {
	api: &'static Api,
	context: *mut c_void,
	packet: *mut PacketHead,
	frame: *mut FrameHead,
	scratch: Vec<u8>,
	extradata: Vec<u8>,
	pending_extradata: bool,
	channels: usize,
}

unsafe impl Send for FfAudioDecoder {}

impl FfAudioDecoder {
	pub fn open(name: &CStr, channels: usize, extradata: Option<&[u8]>) -> Result<Self> {
		let api = api().ok_or_else(|| PlayerError::Unsupported("codec pack unavailable".to_owned()))?;
		let codec = unsafe { (api.find_decoder)(name.as_ptr()) };
		if codec.is_null() {
			return Err(PlayerError::Unsupported(format!(
				"codec pack has no {} decoder",
				name.to_string_lossy()
			)));
		}
		let extradata = extradata.map(<[u8]>::to_vec).unwrap_or_default();
		let decoder = Self {
			api,
			context: unsafe { (api.alloc_context)(codec) },
			packet: unsafe { (api.packet_alloc)() },
			frame: unsafe { (api.frame_alloc)() },
			scratch: Vec::new(),
			pending_extradata: !extradata.is_empty(),
			extradata,
			channels: channels.clamp(1, MAX_PLANES),
		};
		if decoder.context.is_null() || decoder.packet.is_null() || decoder.frame.is_null() {
			return Err(PlayerError::Audio("codec pack allocation failed".to_owned()));
		}
		let opened = unsafe { (api.open)(decoder.context, codec, null_mut()) };
		if opened < 0 {
			return Err(PlayerError::Audio(format!(
				"codec pack could not open an audio decoder ({opened})"
			)));
		}
		Ok(decoder)
	}

	pub fn channels(&self) -> usize {
		self.channels
	}

	pub fn decode(&mut self, data: &[u8], out: &mut Vec<f32>) -> Result<()> {
		self.scratch.clear();
		self.scratch.reserve(data.len() + PADDING);
		self.scratch.extend_from_slice(data);
		self.scratch.resize(data.len() + PADDING, 0);
		if std::mem::take(&mut self.pending_extradata) {
			let target =
				unsafe { (self.api.packet_side_data)(self.packet, NEW_EXTRADATA, self.extradata.len()) };
			if !target.is_null() {
				unsafe {
					std::ptr::copy_nonoverlapping(self.extradata.as_ptr(), target, self.extradata.len())
				};
			}
		} else {
			unsafe { (self.api.packet_free_side_data)(self.packet) };
		}
		loop {
			let sent = unsafe {
				(*self.packet).data = self.scratch.as_mut_ptr();
				(*self.packet).size = data.len() as c_int;
				let sent = (self.api.send_packet)(self.context, self.packet);
				(*self.packet).data = null_mut();
				(*self.packet).size = 0;
				sent
			};
			self.receive(out)?;
			match sent {
				code if code == -EAGAIN => continue,
				code if code < 0 => {
					return Err(PlayerError::Audio(format!(
						"codec pack rejected an audio packet ({code})"
					)));
				}
				_ => return Ok(()),
			}
		}
	}

	pub fn reset(&mut self) {
		unsafe { (self.api.flush)(self.context) };
	}

	fn receive(&mut self, out: &mut Vec<f32>) -> Result<()> {
		loop {
			let received = unsafe { (self.api.receive_frame)(self.context, self.frame) };
			if received == -EAGAIN || received == AVERROR_EOF {
				return Ok(());
			}
			if received < 0 {
				return Err(PlayerError::Audio(format!(
					"codec pack audio decode failed ({received})"
				)));
			}
			self.emit(out);
			unsafe { (self.api.frame_unref)(self.frame) };
		}
	}

	fn emit(&self, out: &mut Vec<f32>) {
		let frame = unsafe { &*self.frame };
		let frames = frame.nb_samples.max(0) as usize;
		let channels = self.channels;
		let format = frame.format;
		let bytes = bytes_per_sample(format);
		if frames == 0 || channels == 0 || bytes == 0 {
			return;
		}
		let planar = is_planar(format);
		let planes = if planar { channels } else { 1 };
		let stride = if planar { frames } else { frames * channels };
		let mut views = [&[][..]; MAX_PLANES];
		for (index, view) in views.iter_mut().enumerate().take(planes) {
			let pointer = frame.data[index].cast_const();
			if pointer.is_null() {
				return;
			}
			*view = unsafe { std::slice::from_raw_parts(pointer, stride * bytes) };
		}
		out.reserve(frames * channels);
		for at in 0..frames {
			for channel in 0..channels {
				let source = if planar { channel } else { 0 };
				let index = if planar { at } else { at * channels + channel };
				out.push(sample_at(views[source], index, format));
			}
		}
	}
}

impl Drop for FfAudioDecoder {
	fn drop(&mut self) {
		unsafe {
			(self.api.frame_free)(&mut self.frame);
			(self.api.packet_free)(&mut self.packet);
			(self.api.free_context)(&mut self.context);
		}
	}
}
