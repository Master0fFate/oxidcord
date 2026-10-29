#[cfg(windows)]
use std::sync::OnceLock;
#[cfg(windows)]
use std::sync::mpsc::Sender;

const NORMAL_RATE: f32 = 1.0;
const FULL_VOLUME: f32 = 1.0;

#[cfg(windows)]
struct Utterance {
	text: String,
	rate: f32,
	volume: f32,
}

#[cfg(windows)]
static SPEAKER: OnceLock<Option<Sender<Utterance>>> = OnceLock::new();

pub fn available() -> bool {
	cfg!(windows)
}

pub fn speak(text: &str) {
	speak_with(text, NORMAL_RATE, FULL_VOLUME);
}

#[cfg(not(windows))]
pub fn speak_with(_text: &str, _rate: f32, _volume: f32) -> bool {
	false
}

#[cfg(windows)]
pub fn speak_with(text: &str, rate: f32, volume: f32) -> bool {
	let Some(speaker) = SPEAKER.get_or_init(start) else {
		return false;
	};
	let utterance = Utterance {
		text: text.to_owned(),
		rate,
		volume,
	};
	if speaker.send(utterance).is_err() {
		tracing::warn!("the speech worker stopped");
		return false;
	}
	true
}

#[cfg(windows)]
fn sapi_rate(rate: f32) -> i32 {
	let steps = if rate >= NORMAL_RATE {
		(rate - NORMAL_RATE) * 10.0
	} else {
		(rate - NORMAL_RATE) * 20.0
	};
	(steps.round() as i32).clamp(-10, 10)
}

#[cfg(windows)]
fn sapi_volume(volume: f32) -> u16 {
	(volume.clamp(0.0, FULL_VOLUME) * 100.0).round() as u16
}

#[cfg(windows)]
fn start() -> Option<Sender<Utterance>> {
	use windows::Win32::Media::Speech::{ISpVoice, SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SpVoice};
	use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
	use windows::core::HSTRING;

	let (sender, receiver) = std::sync::mpsc::channel::<Utterance>();
	let spawned = std::thread::Builder::new().name("speech".into()).spawn(move || {
		if let Err(error) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
			tracing::warn!(%error, "could not start COM for speech");
			return;
		}
		let voice: ISpVoice = match unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) } {
			Ok(voice) => voice,
			Err(error) => {
				tracing::warn!(%error, "no speech voice is installed");
				return;
			}
		};
		let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32;
		for utterance in receiver {
			let configured = unsafe { voice.SetRate(sapi_rate(utterance.rate)) }
				.and_then(|()| unsafe { voice.SetVolume(sapi_volume(utterance.volume)) });
			if let Err(error) = configured {
				tracing::warn!(%error, "could not configure the speech voice");
			}
			if let Err(error) = unsafe { voice.Speak(&HSTRING::from(utterance.text), flags, None) } {
				tracing::warn!(%error, "could not speak the message");
			}
		}
	});
	match spawned {
		Ok(_) => Some(sender),
		Err(error) => {
			tracing::warn!(%error, "could not start the speech worker");
			None
		}
	}
}
