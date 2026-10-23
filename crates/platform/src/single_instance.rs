use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use interprocess::local_socket::traits::{ListenerExt as _, Stream as _};
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, Name, Stream, ToNsName};

use crate::Result;

const MAX_PAYLOAD_BYTES: u64 = 64 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("another instance is already running")]
pub struct AlreadyRunning;

pub struct Guard {
	listener: Option<Listener>,
}

impl Guard {
	pub fn incoming(&mut self) -> Receiver<Vec<u8>> {
		let (sender, receiver) = mpsc::channel();
		if let Some(listener) = self.listener.take() {
			thread::spawn(move || serve(listener, sender));
		}
		receiver
	}
}

pub fn acquire(app_id: &str) -> std::result::Result<Guard, AlreadyRunning> {
	match socket_name(app_id).and_then(bind) {
		Ok(listener) => Ok(Guard {
			listener: Some(listener),
		}),
		Err(error) if name_taken(&error) => Err(AlreadyRunning),
		Err(error) => {
			tracing::warn!(%error, "single instance lock unavailable");
			Ok(Guard { listener: None })
		}
	}
}

pub fn notify_existing(app_id: &str, payload: &[u8]) -> Result<()> {
	let mut stream = Stream::connect(socket_name(app_id)?)?;
	stream.write_all(payload)?;
	Ok(())
}

fn bind(name: Name<'static>) -> io::Result<Listener> {
	match ListenerOptions::new().name(name.clone()).create_sync() {
		Err(error) if error.kind() == io::ErrorKind::AddrInUse && Stream::connect(name.clone()).is_err() => {
			ListenerOptions::new()
				.name(name)
				.try_overwrite(true)
				.create_sync()
		}
		result => result,
	}
}

fn name_taken(error: &io::Error) -> bool {
	matches!(
		error.kind(),
		io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
	)
}

fn socket_name(app_id: &str) -> io::Result<Name<'static>> {
	let user = std::env::var("USERNAME")
		.or_else(|_| std::env::var("USER"))
		.unwrap_or_default();
	format!("{app_id}-{user}")
		.chars()
		.map(|c| {
			if c.is_alphanumeric() || matches!(c, '-' | '_') {
				c
			} else {
				'_'
			}
		})
		.collect::<String>()
		.to_ns_name::<GenericNamespaced>()
}

fn serve(listener: Listener, sender: Sender<Vec<u8>>) {
	for connection in listener.incoming().flatten() {
		let sender = sender.clone();
		thread::spawn(move || {
			let mut payload = Vec::new();
			if connection
				.take(MAX_PAYLOAD_BYTES)
				.read_to_end(&mut payload)
				.is_ok()
			{
				let _ = sender.send(payload);
			}
		});
	}
}
