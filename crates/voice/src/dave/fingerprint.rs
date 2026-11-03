use scrypt::Params;

use super::code::pairwise_display;
use super::session::DaveMember;

const VERSION: [u8; 2] = 0u16.to_be_bytes();
const SALT: [u8; 16] = [
	0x24, 0xca, 0xb1, 0x7a, 0x7a, 0xf8, 0xec, 0x2b, 0x82, 0xb4, 0x12, 0xb9, 0x2d, 0xab, 0x19, 0x2e,
];
const LOG_N: u8 = 14;
const BLOCK_SIZE: u32 = 8;
const PARALLELISM: u32 = 2;
const OUTPUT_BYTES: usize = 64;

pub fn pairwise_code(own: &DaveMember, other: &DaveMember) -> Option<String> {
	let mut passphrase = [identity_buffer(own), identity_buffer(other)];
	passphrase.sort_unstable();
	let params = Params::new(LOG_N, BLOCK_SIZE, PARALLELISM).ok()?;
	let mut fingerprint = [0u8; OUTPUT_BYTES];
	scrypt::scrypt(&passphrase.concat(), &SALT, &params, &mut fingerprint).ok()?;
	pairwise_display(&fingerprint)
}

fn identity_buffer(member: &DaveMember) -> Vec<u8> {
	[
		VERSION.as_slice(),
		&member.signature_key,
		&member.user_id.to_be_bytes(),
	]
	.concat()
}
