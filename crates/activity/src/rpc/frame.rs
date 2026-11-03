use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) const HANDSHAKE: u32 = 0;
pub(crate) const FRAME: u32 = 1;
pub(crate) const CLOSE: u32 = 2;
pub(crate) const PING: u32 = 3;
pub(crate) const PONG: u32 = 4;

const MAX_PAYLOAD_BYTES: u32 = 1024 * 1024;
const HEADER_BYTES: usize = 8;
const READ_CHUNK_BYTES: usize = 4096;

pub(crate) struct Reader<R> {
	inner: R,
	buffer: Vec<u8>,
}

impl<R: AsyncRead + Unpin> Reader<R> {
	pub fn new(inner: R) -> Self {
		Self {
			inner,
			buffer: Vec::new(),
		}
	}

	pub async fn next(&mut self) -> io::Result<Option<(u32, Vec<u8>)>> {
		loop {
			if let Some(frame) = self.take()? {
				return Ok(Some(frame));
			}
			self.buffer.reserve(READ_CHUNK_BYTES);
			if self.inner.read_buf(&mut self.buffer).await? == 0 {
				return Ok(None);
			}
		}
	}

	fn take(&mut self) -> io::Result<Option<(u32, Vec<u8>)>> {
		let Some(header) = self.buffer.first_chunk::<HEADER_BYTES>() else {
			return Ok(None);
		};
		let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
		let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
		if length > MAX_PAYLOAD_BYTES {
			return Err(io::Error::new(io::ErrorKind::InvalidData, "rpc frame too large"));
		}
		let end = HEADER_BYTES + length as usize;
		if self.buffer.len() < end {
			return Ok(None);
		}
		let payload = self.buffer[HEADER_BYTES..end].to_vec();
		self.buffer.drain(..end);
		Ok(Some((opcode, payload)))
	}
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
