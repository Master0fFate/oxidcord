use protocol::{Token, User};
use rest::{RestClient, RestConfig};
use url::Url;
use zeroize::Zeroizing;

use crate::error::{LoginError, Result};

pub struct TokenLogin;

pub(crate) fn clean_token(input: &str) -> &str {
	input
		.trim()
		.trim_matches(|c| matches!(c, '"' | '\'' | '`'))
		.trim()
}

pub(crate) fn plausible(token: &str) -> bool {
	!token.is_empty() && token.len() <= 512 && token.bytes().all(|byte| byte.is_ascii_graphic())
}

impl TokenLogin {
	pub async fn validate(rest_base: Url, token: String) -> Result<(Token, User)> {
		let pasted = Zeroizing::new(token);
		let cleaned = clean_token(&pasted);
		if !plausible(cleaned) {
			return Err(LoginError::InvalidToken);
		}
		let client = RestClient::new(RestConfig {
			base_url: rest_base,
			..RestConfig::default()
		});
		client.set_token(Token::new(cleaned));
		let user = client.current_user().await;
		client.clear_token();
		Ok((Token::new(cleaned), user?))
	}
}
