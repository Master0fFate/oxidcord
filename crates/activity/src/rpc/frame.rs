use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) const HANDSHAKE: u32 = 0;
pub(crate) const FRAME: u32 = 1;
pub(crate) const CLOSE: u32 = 2;
pub(crate) const PING: u32 = 3;
pub(crate) const PONG: u32 = 4;

const MAX_PAYLOAD_BYTES: u32 = 1024 * 1024;

pub(crate) async fn read<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<(u32, Vec<u8>)>> {
	let mut header = [0u8; 8];
	match reader.read_exact(&mut header).await {
		Ok(_) => {}
		Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
		Err(error) => return Err(error),
	}
	let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
	let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
	if length > MAX_PAYLOAD_BYTES {
		return Err(io::Error::new(io::ErrorKind::InvalidData, "rpc frame too large"));
	}
	let mut payload = vec![0u8; length as usize];
	reader.read_exact(&mut payload).await?;
	Ok(Some((opcode, payload)))
}

pub(crate) async fn write<W: AsyncWrite + Unpin>(
	writer: &mut W,
	opcode: u32,
	payload: &[u8],
) -> io::Result<()> {
	let mut frame = Vec::with_capacity(8 + payload.len());
	frame.extend_from_slice(&opcode.to_le_bytes());
	frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
	frame.extend_from_slice(payload);
	writer.write_all(&frame).await?;
	writer.flush().await
}
