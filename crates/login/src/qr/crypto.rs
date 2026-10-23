use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_PAD_INDIFFERENT, URL_SAFE_NO_PAD};
use protocol::Token;
use rsa::pkcs8::EncodePublicKey;
use rsa::rand_core::OsRng;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::error::{LoginError, Result};

const KEY_BITS: usize = 2048;

pub(super) struct SessionKey {
	private: RsaPrivateKey,
	public_der: Vec<u8>,
}

fn crypto_error(reason: &str) -> LoginError {
	LoginError::Protocol(reason.to_owned())
}

impl SessionKey {
	pub fn generate() -> Result<Self> {
		let private =
			RsaPrivateKey::new(&mut OsRng, KEY_BITS).map_err(|_| crypto_error("key generation failed"))?;
		let public_der = RsaPublicKey::from(&private)
			.to_public_key_der()
			.map_err(|_| crypto_error("public key encoding failed"))?
			.into_vec();
		Ok(Self { private, public_der })
	}

	pub fn encoded_public_key(&self) -> String {
		STANDARD.encode(&self.public_der)
	}

	pub fn fingerprint(&self) -> String {
		URL_SAFE_NO_PAD.encode(Sha256::digest(&self.public_der))
	}

	pub fn decrypt(&self, encoded: &str) -> Result<Zeroizing<Vec<u8>>> {
		let ciphertext = STANDARD_PAD_INDIFFERENT
			.decode(encoded)
			.map_err(|_| crypto_error("ciphertext is not base64"))?;
		self.private
			.decrypt(Oaep::new::<Sha256>(), &ciphertext)
			.map(Zeroizing::new)
			.map_err(|_| crypto_error("decryption failed"))
	}

	pub fn decrypt_token(&self, encoded: &str) -> Result<Token> {
		let plain = self.decrypt(encoded)?;
		let text = std::str::from_utf8(&plain).map_err(|_| crypto_error("token is not utf-8"))?;
		Ok(Token::new(text))
	}
}

pub(super) fn nonce_proof(nonce: &[u8]) -> String {
	URL_SAFE_NO_PAD.encode(nonce)
}
