use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::atomic::write_atomic;
use crate::error::Result;

const KEYRING_SERVICE: &str = "oxidcord";
const KEYRING_USER: &str = "session-token";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenBackend {
	Keyring,
	PlaintextFile,
}

#[derive(Debug)]
pub(crate) struct SecretSlot {
	keyring_user: String,
	file_path: PathBuf,
}

impl SecretSlot {
	pub(crate) fn new(keyring_user: impl Into<String>, file_path: PathBuf) -> Self {
		Self {
			keyring_user: keyring_user.into(),
			file_path,
		}
	}

	pub(crate) fn save(&self, token: &str, plaintext: bool) -> Result<TokenBackend> {
		if !plaintext {
			match self.save_to_keyring(token) {
				Ok(()) => {
					self.remove_file();
					return Ok(TokenBackend::Keyring);
				}
				Err(error) => tracing::warn!(%error, "keyring unavailable, storing token in a file"),
			}
		}
		write_atomic(&self.file_path, token.as_bytes(), true)?;
		Ok(TokenBackend::PlaintextFile)
	}

	pub(crate) fn load(&self, plaintext: bool) -> Result<Option<String>> {
		let mut keyring_failure = None;
		if !plaintext {
			match self.load_from_keyring() {
				Ok(Some(token)) => return Ok(Some(token)),
				Ok(None) => {}
				Err(error) => keyring_failure = Some(error),
			}
		}
		match fs::read_to_string(&self.file_path) {
			Ok(token) => Ok(Some(token)),
			Err(error) if error.kind() == ErrorKind::NotFound => keyring_failure.map_or(Ok(None), Err),
			Err(error) => Err(error.into()),
		}
	}

	pub(crate) fn clear(&self) {
		if let Err(error) = self.delete_from_keyring() {
			tracing::warn!(%error, "could not remove the token from the keyring");
		}
		self.remove_file();
	}

	fn remove_file(&self) {
		if let Err(error) = fs::remove_file(&self.file_path)
			&& error.kind() != ErrorKind::NotFound
		{
			tracing::warn!(%error, "could not remove the token file");
		}
	}

	fn keyring_entry(&self) -> Result<keyring::Entry> {
		Ok(keyring::Entry::new(KEYRING_SERVICE, &self.keyring_user)?)
	}

	fn save_to_keyring(&self, token: &str) -> Result<()> {
		self.keyring_entry()?.set_password(token)?;
		Ok(())
	}

	fn load_from_keyring(&self) -> Result<Option<String>> {
		match self.keyring_entry()?.get_password() {
			Ok(token) => Ok(Some(token)),
			Err(keyring::Error::NoEntry) => Ok(None),
			Err(error) => Err(error.into()),
		}
	}

	fn delete_from_keyring(&self) -> Result<()> {
		match self.keyring_entry()?.delete_credential() {
			Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
			Err(error) => Err(error.into()),
		}
	}
}

#[derive(Debug)]
pub struct TokenStore {
	slot: SecretSlot,
	plaintext: AtomicBool,
}

impl TokenStore {
	pub fn new(file_path: PathBuf, plaintext: bool) -> Self {
		Self {
			slot: SecretSlot::new(KEYRING_USER, file_path),
			plaintext: AtomicBool::new(plaintext),
		}
	}

	pub fn set_plaintext(&self, plaintext: bool) {
		self.plaintext.store(plaintext, Ordering::Relaxed);
	}

	pub fn save(&self, token: &str) -> Result<TokenBackend> {
		self.slot.save(token, self.plaintext.load(Ordering::Relaxed))
	}

	pub fn load(&self) -> Result<Option<String>> {
		self.slot.load(self.plaintext.load(Ordering::Relaxed))
	}

	pub fn clear(&self) {
		self.slot.clear();
	}
}
