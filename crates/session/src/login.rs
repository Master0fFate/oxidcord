use protocol::{Token, User};
use rest::{RestClient, RestConfig, RestError};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum LoginError {
	#[error("token was rejected")]
	Rejected,
	#[error("token is empty")]
	Empty,
	#[error(transparent)]
	Rest(RestError),
}

pub enum LoginMethod {
	Token(String),
}

pub struct Login {
	pub token: Token,
	pub user: User,
}

impl LoginMethod {
	pub async fn authenticate(self, rest_base: &Url) -> Result<Login, LoginError> {
		match self {
			Self::Token(token) => {
				let token = token.trim().to_owned();
				let user = validate_token(rest_base, &token).await?;
				Ok(Login {
					token: Token::new(token),
					user,
				})
			}
		}
	}
}

pub async fn validate_token(rest_base: &Url, token: &str) -> Result<User, LoginError> {
	if token.is_empty() {
		return Err(LoginError::Empty);
	}
	let client = RestClient::new(RestConfig {
		base_url: rest_base.clone(),
		..RestConfig::default()
	});
	client.set_token(Token::new(token));
	client.current_user().await.map_err(|error| match error {
		RestError::Unauthorized | RestError::Forbidden => LoginError::Rejected,
		other => LoginError::Rest(other),
	})
}
