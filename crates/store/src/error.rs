use zeroize::Zeroize;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
	#[error("io error: {0}")]
	Io(#[from] std::io::Error),
	#[error("sqlite error: {0}")]
	Sqlite(#[from] rusqlite::Error),
	#[error("json error: {0}")]
	Json(#[from] serde_json::Error),
	#[error("keyring error: {0}")]
	Keyring(&'static str),
	#[error("cache encryption error: {0}")]
	Seal(&'static str),
	#[error("database schema version {found} is newer than supported version {supported}")]
	SchemaTooNew { found: u32, supported: u32 },
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl From<keyring::Error> for StoreError {
	fn from(error: keyring::Error) -> Self {
		let reason = match error {
			keyring::Error::NoEntry => "no matching credential",
			keyring::Error::NoStorageAccess(_) => "credential storage is not accessible",
			keyring::Error::PlatformFailure(_) => "platform failure",
			keyring::Error::NoDefaultStore => "no credential store available",
			keyring::Error::BadEncoding(mut bytes) => {
				bytes.zeroize();
				"credential is not valid utf-8"
			}
			keyring::Error::BadDataFormat(mut bytes, _) => {
				bytes.zeroize();
				"credential has a bad data format"
			}
			_ => "credential store rejected the request",
		};
		Self::Keyring(reason)
	}
}
