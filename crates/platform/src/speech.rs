use std::sync::OnceLock;
use std::sync::mpsc::Sender;

static SPEAKER: OnceLock<Option<Sender<String>>> = OnceLock::new();

pub fn available() -> bool {
	cfg!(windows)
}

pub fn speak(text: &str) {
	let speaker = SPEAKER.get_or_init(start);
	if let Some(speaker) = speaker
		&& speaker.send(text.to_owned()).is_err()
	{
		tracing::warn!("the speech worker stopped");
	}
}

#[cfg(windows)]
fn start() -> Option<Sender<String>> {
	use windows::Win32::Media::Speech::{ISpVoice, SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SpVoice};
	use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
	use windows::core::HSTRING;

	let (sender, receiver) = std::sync::mpsc::channel::<String>();
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
		for text in receiver {
			if let Err(error) = unsafe { voice.Speak(&HSTRING::from(text), flags, None) } {
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

#[cfg(not(windows))]
fn start() -> Option<Sender<String>> {
	None
}
