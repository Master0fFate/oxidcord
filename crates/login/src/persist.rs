use protocol::Token;
use store::{Store, TokenBackend};
use zeroize::Zeroizing;

use crate::error::Result;
use crate::token_login::clean_token;

pub fn save_token(store: &Store, token: &Token) -> Result<TokenBackend> {
	Ok(store.tokens.save(token.as_str())?)
}

pub fn load_token(store: &Store) -> Option<Token> {
	match store.tokens.load() {
		Ok(stored) => {
			let stored = Zeroizing::new(stored?);
			let cleaned = clean_token(&stored);
			(!cleaned.is_empty()).then(|| Token::new(cleaned))
		}
		Err(error) => {
			tracing::warn!(%error, "could not read the stored token");
			None
		}
	}
}
