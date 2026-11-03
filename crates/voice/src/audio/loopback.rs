use std::mem::{ManuallyDrop, size_of};
use std::sync::mpsc;
use std::sync::{Arc, atomic::Ordering};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
	AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
	AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
	AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
	AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
	ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
	IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
	IAudioCaptureClient, IAudioClient, PROCESS_LOOPBACK_MODE,
	PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
	VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::{
	PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, IAgileObject};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects};
use windows::Win32::System::Variant::VT_BLOB;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::core::{IUnknown, Interface, Ref, implement};

use super::Rings;
use crate::codec::SAMPLE_RATE;
use crate::error::{Result, VoiceError};
use crate::video::capture::{ScreenTarget, ScreenTargetKind};

const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const CHANNELS: u16 = 2;
const SAMPLE_BYTES: u16 = 4;
const BUFFER_HUNDRED_NANOS: i64 = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SystemScope {
	Only(u32),
	Except(u32),
}

impl SystemScope {
	pub fn for_target(target: &ScreenTarget) -> Self {
		let own = std::process::id();
		if target.kind != ScreenTargetKind::Window {
			return Self::Except(own);
		}
		let mut process = 0;
		unsafe { GetWindowThreadProcessId(HWND(target.id as isize as *mut _), Some(&mut process)) };
		if process == 0 {
			Self::Except(own)
		} else {
			Self::Only(process)
		}
	}

	fn activation(self) -> AUDIOCLIENT_ACTIVATION_PARAMS {
		let (process, mode): (u32, PROCESS_LOOPBACK_MODE) = match self {
			Self::Only(process) => (process, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE),
			Self::Except(process) => (process, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE),
		};
		AUDIOCLIENT_ACTIVATION_PARAMS {
			ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
			Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
				ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
					TargetProcessId: process,
					ProcessLoopbackMode: mode,
				},
			},
		}
	}
}

fn failure(error: windows::core::Error) -> VoiceError {
	VoiceError::Audio(error.message())
}

struct Event(HANDLE);

unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
	fn new() -> Result<Self> {
		unsafe { CreateEventW(None, false, false, None) }
			.map(Self)
			.map_err(failure)
	}

	fn signal(&self) {
		let _ = unsafe { SetEvent(self.0) };
	}
}

impl Drop for Event {
	fn drop(&mut self) {
		let _ = unsafe { CloseHandle(self.0) };
	}
}

struct ComScope;

impl ComScope {
	fn enter() -> Self {
		let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
		Self
	}
}

impl Drop for ComScope {
	fn drop(&mut self) {
		unsafe { CoUninitialize() };
	}
}

#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Completion(mpsc::Sender<()>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
	fn ActivateCompleted(&self, _: Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
		let _ = self.0.send(());
		Ok(())
	}
}

impl windows::Win32::System::Com::IAgileObject_Impl for Completion_Impl {}

pub(super) struct ProcessLoopback {
	stop: Arc<Event>,
	thread: Option<JoinHandle<()>>,
}

impl ProcessLoopback {
	pub fn start(scope: SystemScope, rings: &Arc<Rings>) -> Result<Self> {
		let stop = Arc::new(Event::new()?);
		let (ready_sender, ready) = mpsc::channel();
		let thread = thread::Builder::new()
			.name("voice-loopback".to_owned())
			.spawn({
				let rings = rings.clone();
				let stop = stop.clone();
				move || capture(scope, &rings, &stop, &ready_sender)
			})
			.map_err(|error| VoiceError::Audio(error.to_string()))?;
		let started = ready
			.recv()
			.unwrap_or_else(|_| Err(VoiceError::Audio("audio capture stopped".to_owned())));
		match started {
			Ok(()) => Ok(Self {
				stop,
				thread: Some(thread),
			}),
			Err(error) => {
				let _ = thread.join();
				Err(error)
			}
		}
	}
}

impl Drop for ProcessLoopback {
	fn drop(&mut self) {
		self.stop.signal();
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}
}

fn capture(scope: SystemScope, rings: &Arc<Rings>, stop: &Event, ready: &mpsc::Sender<Result<()>>) {
	let _com = ComScope::enter();
	let opened = open(scope);
	let (client, reader, wake) = match opened {
		Ok(opened) => opened,
		Err(error) => {
			let _ = ready.send(Err(error));
			return;
		}
	};
	rings.system_rate.store(SAMPLE_RATE, Ordering::Release);
	let _ = ready.send(Ok(()));
	let handles = [wake.0, stop.0];
	while unsafe { WaitForMultipleObjects(&handles, false, INFINITE) } == WAIT_OBJECT_0 {
		if let Err(error) = drain(&reader, rings) {
			tracing::warn!(%error, "process audio capture stopped");
			break;
		}
	}
	let _ = unsafe { client.Stop() };
}

fn open(scope: SystemScope) -> Result<(IAudioClient, IAudioCaptureClient, Event)> {
	let client = activate(scope)?;
	let format = WAVEFORMATEX {
		wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
		nChannels: CHANNELS,
		nSamplesPerSec: SAMPLE_RATE,
		nAvgBytesPerSec: SAMPLE_RATE * u32::from(CHANNELS * SAMPLE_BYTES),
		nBlockAlign: CHANNELS * SAMPLE_BYTES,
		wBitsPerSample: SAMPLE_BYTES * 8,
		cbSize: 0,
	};
	let wake = Event::new()?;
	unsafe {
		client
			.Initialize(
				AUDCLNT_SHAREMODE_SHARED,
				AUDCLNT_STREAMFLAGS_LOOPBACK
					| AUDCLNT_STREAMFLAGS_EVENTCALLBACK
					| AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
					| AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
				BUFFER_HUNDRED_NANOS,
				0,
				&format,
				None,
			)
			.map_err(failure)?;
		client.SetEventHandle(wake.0).map_err(failure)?;
		let reader: IAudioCaptureClient = client.GetService().map_err(failure)?;
		client.Start().map_err(failure)?;
		Ok((client, reader, wake))
	}
}

fn activate(scope: SystemScope) -> Result<IAudioClient> {
	let mut params = scope.activation();
	let argument = ManuallyDrop::new(PROPVARIANT {
		Anonymous: PROPVARIANT_0 {
			Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
				vt: VT_BLOB,
				wReserved1: 0,
				wReserved2: 0,
				wReserved3: 0,
				Anonymous: PROPVARIANT_0_0_0 {
					blob: BLOB {
						cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
						pBlobData: std::ptr::from_mut(&mut params).cast(),
					},
				},
			}),
		},
	});
	let (completed, completion) = mpsc::channel();
	let handler: IActivateAudioInterfaceCompletionHandler = Completion(completed).into();
	let operation = unsafe {
		ActivateAudioInterfaceAsync(
			VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
			&IAudioClient::IID,
			Some(&*argument),
			&handler,
		)
	}
	.map_err(failure)?;
	completion
		.recv()
		.map_err(|_| VoiceError::Audio("audio activation was dropped".to_owned()))?;
	let mut result = windows::core::HRESULT(0);
	let mut activated: Option<IUnknown> = None;
	unsafe { operation.GetActivateResult(&mut result, &mut activated) }.map_err(failure)?;
	result.ok().map_err(failure)?;
	activated
		.ok_or_else(|| VoiceError::Audio("audio activation returned nothing".to_owned()))?
		.cast()
		.map_err(failure)
}

fn drain(reader: &IAudioCaptureClient, rings: &Rings) -> Result<()> {
	loop {
		let pending = unsafe { reader.GetNextPacketSize() }.map_err(failure)?;
		if pending == 0 {
			return Ok(());
		}
		let mut data = std::ptr::null_mut();
		let mut frames = 0;
		let mut flags = 0;
		unsafe { reader.GetBuffer(&mut data, &mut frames, &mut flags, None, None) }.map_err(failure)?;
		let samples = frames as usize * usize::from(CHANNELS);
		if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
			rings.push_system(std::iter::repeat_n(0.0, samples));
		} else {
			let interleaved = unsafe { std::slice::from_raw_parts(data.cast::<f32>(), samples) };
			rings.push_system(interleaved.iter().copied());
		}
		unsafe { reader.ReleaseBuffer(frames) }.map_err(failure)?;
	}
}
