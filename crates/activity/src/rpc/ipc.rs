use std::io;
use std::sync::Arc;
use std::time::Duration;

use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::traits::tokio::Listener as _;
use interprocess::local_socket::{ListenerOptions, Name, Stream, tokio as async_socket};
use serde_json::Value;
use tokio::sync::watch;
use tokio::time::sleep;

use super::RpcShared;
use super::frame;
use super::link::{Close, Link, Transport};

const PIPE_COUNT: u8 = 10;
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

#[cfg(windows)]
pub fn socket_name(index: u8) -> io::Result<Name<'static>> {
	use interprocess::local_socket::{GenericNamespaced, ToNsName};
	format!("discord-ipc-{index}").to_ns_name::<GenericNamespaced>()
}

#[cfg(not(windows))]
pub fn socket_name(index: u8) -> io::Result<Name<'static>> {
	use interprocess::local_socket::{GenericFilePath, ToFsName};
	let base = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
		.iter()
		.find_map(|key| std::env::var_os(key).filter(|value| !value.is_empty()))
		.map(std::path::PathBuf::from)
		.unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
	base.join(format!("discord-ipc-{index}"))
		.to_fs_name::<GenericFilePath>()
}

fn create(name: Name<'static>) -> io::Result<async_socket::Listener> {
	match ListenerOptions::new().name(name.clone()).create_tokio() {
		Err(error) if error.kind() == io::ErrorKind::AddrInUse && Stream::connect(name.clone()).is_err() => {
			ListenerOptions::new()
				.name(name)
				.try_overwrite(true)
				.create_tokio()
		}
		result => result,
	}
}

pub(crate) fn bind() -> Option<(u8, async_socket::Listener)> {
	for index in 0..PIPE_COUNT {
		match socket_name(index).and_then(create) {
			Ok(listener) => return Some((index, listener)),
			Err(error)
				if matches!(
					error.kind(),
					io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
				) => {}
			Err(error) => {
				tracing::warn!(%error, index, "rpc pipe could not be created");
				return None;
			}
		}
	}
	tracing::warn!("all rpc pipes are taken");
	None
}

pub(crate) async fn accept_loop(
	shared: Arc<RpcShared>,
	listener: async_socket::Listener,
	mut enabled: watch::Receiver<bool>,
) {
	loop {
		tokio::select! {
			accepted = listener.accept() => match accepted {
				Ok(stream) => {
					if shared.admit() {
						tokio::spawn(run(shared.clone(), stream, enabled.clone()));
					}
				}
				Err(error) => {
					tracing::debug!(%error, "rpc pipe accept failed");
					sleep(ACCEPT_RETRY).await;
				}
			},
			changed = enabled.changed() => {
				if changed.is_err() || !*enabled.borrow() {
					return;
				}
			}
		}
	}
}

async fn run(shared: Arc<RpcShared>, mut stream: async_socket::Stream, mut enabled: watch::Receiver<bool>) {
	let mut link = Link::new(shared.clone(), Transport::Ipc);
	let outcome = tokio::select! {
		outcome = session(&mut link, &mut stream) => outcome,
		_ = enabled.changed() => Ok(()),
	};
	if let Err(error) = outcome {
		tracing::debug!(%error, "rpc pipe connection ended");
	}
	shared.release();
}

async fn close(stream: &mut async_socket::Stream, close: Close) -> io::Result<()> {
	frame::write(stream, frame::CLOSE, close.payload().to_string().as_bytes()).await
}

async fn session(link: &mut Link, stream: &mut async_socket::Stream) -> io::Result<()> {
	let Some((opcode, payload)) = frame::read(stream).await? else {
		return Ok(());
	};
	if opcode != frame::HANDSHAKE {
		return close(stream, Close::INVALID_CLIENT_ID).await;
	}
	let Ok(handshake) = serde_json::from_slice::<Value>(&payload) else {
		return close(stream, Close::INVALID_ENCODING).await;
	};
	let version = handshake.get("v").and_then(Value::as_i64);
	let client_id = handshake.get("client_id").and_then(Value::as_str);
	let ready = match link.handshake(version, client_id, None).await {
		Ok(ready) => ready,
		Err(reason) => return close(stream, reason).await,
	};
	frame::write(stream, frame::FRAME, ready.to_string().as_bytes()).await?;
	while let Some((opcode, payload)) = frame::read(stream).await? {
		match opcode {
			frame::FRAME => {
				let response = match serde_json::from_slice::<Value>(&payload) {
					Ok(command) => link.command(&command).await,
					Err(_) => return close(stream, Close::INVALID_ENCODING).await,
				};
				frame::write(stream, frame::FRAME, response.to_string().as_bytes()).await?;
			}
			frame::PING => frame::write(stream, frame::PONG, &payload).await?,
			frame::PONG => {}
			frame::CLOSE => return Ok(()),
			_ => return close(stream, Close::INVALID_ENCODING).await,
		}
	}
	Ok(())
}
