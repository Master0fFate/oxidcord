mod accounts;
mod atomic;
mod database;
mod error;
pub mod migrations;
mod paths;
mod seal;
mod settings;
mod token;

use parking_lot::RwLock;

pub use accounts::{Account, AccountStore};
pub use atomic::write_atomic;
pub use database::{Database, MediaEntry, ReadState, Table};
pub use error::{Result, StoreError};
pub use paths::Paths;
pub use settings::{
	AccessibilitySettings, AppearanceSettings, ChatSettings, ClientSettings, DeveloperSettings,
	KeybindSettings, NotificationSettings, PluginSettings, Settings, StartupSettings, StorageSettings,
	StreamerModeSettings, VoiceSettings, WindowSettings,
};
pub use token::{TokenBackend, TokenStore};

const ACCOUNTS_FILE: &str = "accounts.json";
const ACCOUNT_TOKENS_DIR: &str = "account-tokens";

pub struct Store {
	pub paths: Paths,
	pub settings: RwLock<Settings>,
	pub db: Database,
	pub tokens: TokenStore,
	pub accounts: AccountStore,
}

impl Store {
	pub fn open(paths: Paths) -> Result<Self> {
		let settings = Settings::load(&paths);
		let cache_key = seal::load_or_create_key(paths.cache_key_path(), settings.storage.plaintext_token)?;
		let db = Database::open(&paths.database_path(), &cache_key)?;
		let tokens = TokenStore::new(paths.token_path(), settings.storage.plaintext_token);
		let accounts = AccountStore::new(
			paths.data_dir().join(ACCOUNTS_FILE),
			paths.config_dir().join(ACCOUNT_TOKENS_DIR),
			settings.storage.plaintext_token,
		);
		Ok(Self {
			paths,
			settings: RwLock::new(settings),
			db,
			tokens,
			accounts,
		})
	}

	pub fn update_settings(&self, apply: impl FnOnce(&mut Settings)) -> Result<()> {
		let mut settings = self.settings.write();
		apply(&mut settings);
		self.tokens.set_plaintext(settings.storage.plaintext_token);
		self.accounts.set_plaintext(settings.storage.plaintext_token);
		settings.save(&self.paths)
	}
}
