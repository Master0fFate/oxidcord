use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use zeroize::Zeroizing;

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
		if plaintext {
			write_atomic(&self.file_path, token.as_bytes(), true)?;
			return Ok(TokenBackend::PlaintextFile);
		}
		self.save_to_keyring(token)?;
		self.remove_file();
		Ok(TokenBackend::Keyring)
	}

	pub(crate) fn load(&self, plaintext: bool) -> Result<Option<String>> {
		if plaintext {
			return match self.load_from_file()? {
				Some(token) => Ok(Some(token)),
				None => Ok(self.load_from_keyring().unwrap_or_default()),
			};
		}
		match self.load_from_keyring() {
			Ok(Some(token)) => Ok(Some(token)),
			Ok(None) => self.load_from_file(),
			Err(error) => self.load_from_file()?.map_or(Err(error), |token| Ok(Some(token))),
		}
	}

	pub(crate) fn migrate(&self, plaintext: bool) -> Result<()> {
		let stored = self.load(!plaintext).unwrap_or_else(|error| {
			tracing::warn!(%error, "could not read a secret to move");
			None
		});
		let Some(secret) = stored.map(Zeroizing::new) else {
			return Ok(());
		};
		self.save(&secret, plaintext)?;
		if plaintext && let Err(error) = self.delete_from_keyring() {
			tracing::warn!(%error, "could not remove a secret from the keyring");
		}
		Ok(())
	}

	fn load_from_file(&self) -> Result<Option<String>> {
		match fs::read_to_string(&self.file_path) {
			Ok(token) => Ok(Some(token)),
			Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
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

	pub(crate) fn migrate(&self, plaintext: bool) -> Result<()> {
		self.slot.migrate(plaintext)
	}
}
