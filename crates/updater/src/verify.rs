use ed25519_dalek::{Signature, VerifyingKey};

use crate::error::{Result, UpdaterError};

const SIGNATURE_BYTES: usize = 64;

pub(crate) fn verify_manifest(public_key: &[u8; 32], manifest: &[u8], signature_text: &[u8]) -> Result<()> {
	let key = VerifyingKey::from_bytes(public_key).map_err(|_| UpdaterError::BadSignature)?;
	let text = std::str::from_utf8(signature_text).map_err(|_| UpdaterError::BadSignature)?;
	let mut bytes = [0u8; SIGNATURE_BYTES];
	hex::decode_to_slice(text.trim(), &mut bytes).map_err(|_| UpdaterError::BadSignature)?;
	key.verify_strict(manifest, &Signature::from_bytes(&bytes))
		.map_err(|_| UpdaterError::BadSignature)
}
