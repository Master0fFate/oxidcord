use rand::Rng;
use rand::rngs::StdRng;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const ACCENT_NIBBLE_BYTES: usize = 12;

fn base64(bytes: &[u8]) -> String {
	let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
	for chunk in bytes.chunks(3) {
		let block = chunk.iter().enumerate().fold(0u32, |acc, (index, byte)| {
			acc | (u32::from(*byte) << (16 - 8 * index))
		});
		for position in 0..4 {
			if position <= chunk.len() {
				let sextet = (block >> (18 - 6 * position)) & 0x3F;
				encoded.push(char::from(ALPHABET[sextet as usize]));
			} else {
				encoded.push('=');
			}
		}
	}
	encoded
}

pub fn thumbhash(rng: &mut StdRng, landscape: bool) -> String {
	let luma = rng.random_range(20..50u32);
	let chroma_p = rng.random_range(24..40u32);
	let chroma_q = rng.random_range(24..40u32);
	let luma_scale = rng.random_range(8..16u32);
	let header24 = luma | (chroma_p << 6) | (chroma_q << 12) | (luma_scale << 18);
	let scale_p = rng.random_range(8..24u32);
	let scale_q = rng.random_range(8..24u32);
	let header16 = 3 | (scale_p << 3) | (scale_q << 9) | (u32::from(landscape) << 15);
	let mut bytes = vec![
		header24 as u8,
		(header24 >> 8) as u8,
		(header24 >> 16) as u8,
		header16 as u8,
		(header16 >> 8) as u8,
	];
	bytes.extend((0..ACCENT_NIBBLE_BYTES).map(|_| rng.random::<u8>()));
	base64(&bytes)
}
