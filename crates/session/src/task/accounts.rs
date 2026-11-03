use std::sync::Arc;

use protocol::{ChannelType, Snowflake, Token};
use store::{Database, Store};

use super::{Core, Internal};

fn known_account(store: &Store, token: &Token) -> Option<Snowflake> {
	match token.user_id() {
		Some(user_id) => store.accounts.token_for(user_id).ok().flatten().map(|_| user_id),
		None => store.accounts.current(),
	}
}

impl Core {
	pub(super) fn refresh_token(&mut self, token: Token) {
		self.rest.set_token(Token::new(token.as_str()));
		let previous = std::mem::replace(&mut self.token, Arc::new(token));
		self.account_saved = false;
		self.remember_account();
		let store = self.store.clone();
		let token = self.token.clone();
		self.runtime.spawn_blocking(move || {
			if store.tokens.load().ok().flatten().as_deref() != Some(previous.as_str()) {
				return;
			}
			if let Err(error) = store.tokens.save(token.as_str()) {
				tracing::warn!(%error, "rotated token was not saved");
			}
		});
	}

	pub(super) fn remember_account(&mut self) {
		if self.account_saved {
			return;
		}
		let Some(user) = self.current_user() else {
			return;
		};
		self.account_saved = true;
		let store = self.store.clone();
		let token = self.token.clone();
		self.runtime.spawn_blocking(move || {
			let saved = store
				.accounts
				.save(user.id, &user.username, user.avatar.as_deref(), token.as_str())
				.and_then(|_| store.accounts.set_current(user.id));
			if let Err(error) = saved {
				tracing::warn!(%error, "account was not saved");
			}
		});
	}

	pub(super) async fn open_current_cache(&mut self) -> Option<Arc<Database>> {
		let store = self.store.clone();
		let token = self.token.clone();
		let opened = tokio::task::spawn_blocking(move || {
			let user_id = known_account(&store, &token)?;
			match store.open_account_cache(user_id) {
				Ok(db) => Some((user_id, db)),
				Err(error) => {
					tracing::warn!(%error, "could not open the account cache");
					None
				}
			}
		})
		.await;
		let Ok(Some((user_id, db))) = opened else {
			return None;
		};
		let db = Arc::new(db);
		self.persist.attach(user_id, Arc::clone(&db));
		Some(db)
	}

	pub(super) fn attach_cache(&mut self, user_id: Snowflake) {
		if self.persist.owner() == Some(user_id) || self.persist.is_opening(user_id) {
			return;
		}
		self.persist.begin_open(user_id);
		let store = self.store.clone();
		let internal = self.internal.clone();
		self.runtime.spawn_blocking(move || {
			let db = match store.open_account_cache(user_id) {
				Ok(db) => Some(Arc::new(db)),
				Err(error) => {
					tracing::warn!(%error, "could not open the account cache");
					None
				}
			};
			let _ = internal.send(Internal::CacheOpened { user_id, db });
		});
	}

	pub(super) fn on_cache_opened(&mut self, user_id: Snowflake, db: Option<Arc<Database>>) {
		if !self.persist.is_opening(user_id) {
			return;
		}
		match db {
			Some(db) => {
				self.persist.attach(user_id, db);
				self.restore_recents();
				if self.state.read().selected().is_none() {
					self.restore_selection();
				}
			}
			None => self.persist.detach(),
		}
	}

	pub(super) fn forget_account(&self) {
		let Some(user_id) = self.state.read().current_user_id() else {
			return;
		};
		if let Err(error) = self.store.accounts.remove(user_id) {
			tracing::warn!(%error, "account was not removed");
		}
	}

	pub(super) fn select_new_guild(&mut self, guild_id: Snowflake) {
		if !std::mem::take(&mut self.select_created_guild) {
			return;
		}
		let first_text = self.state.read().guild(guild_id).and_then(|guild| {
			guild
				.channels
				.values()
				.filter(|channel| channel.kind == ChannelType::GuildText)
				.min_by_key(|channel| channel.position)
				.map(|channel| channel.id)
		});
		if let Some(channel_id) = first_text {
			self.select_channel(Some(guild_id), channel_id);
		}
	}

	pub(super) fn switch_account(&self, user_id: Snowflake) {
		let store = self.store.clone();
		let internal = self.internal.clone();
		self.runtime.spawn_blocking(move || {
			let message = match store.accounts.token_for(user_id) {
				Ok(Some(_)) => Internal::AccountSwitch(user_id),
				Ok(None) => Internal::Rejected("That account has no saved sign-in".to_owned()),
				Err(error) => {
					tracing::warn!(%error, "account sign-in was not read");
					Internal::Rejected("That account could not be read".to_owned())
				}
			};
			let _ = internal.send(message);
		});
	}
}
