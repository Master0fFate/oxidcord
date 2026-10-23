use rusqlite::Connection;

use crate::error::{Result, StoreError};

const MIGRATIONS: &[&str] = &["
	CREATE TABLE kv (
		key TEXT PRIMARY KEY,
		value BLOB NOT NULL,
		updated_at INTEGER NOT NULL
	);
	CREATE TABLE guilds (
		id INTEGER PRIMARY KEY,
		json BLOB NOT NULL,
		updated_at INTEGER NOT NULL
	);
	CREATE TABLE channels (
		id INTEGER PRIMARY KEY,
		guild_id INTEGER,
		json BLOB NOT NULL,
		updated_at INTEGER NOT NULL
	);
	CREATE INDEX channels_guild_id ON channels (guild_id);
	CREATE TABLE users (
		id INTEGER PRIMARY KEY,
		json BLOB NOT NULL,
		updated_at INTEGER NOT NULL
	);
	CREATE TABLE messages (
		id INTEGER PRIMARY KEY,
		channel_id INTEGER NOT NULL,
		json BLOB NOT NULL,
		updated_at INTEGER NOT NULL
	);
	CREATE INDEX messages_channel_id_id ON messages (channel_id, id DESC);
	CREATE TABLE read_state (
		channel_id INTEGER PRIMARY KEY,
		last_message_id INTEGER,
		mention_count INTEGER NOT NULL DEFAULT 0,
		last_viewed INTEGER
	);
	CREATE TABLE drafts (
		channel_id INTEGER PRIMARY KEY,
		content TEXT NOT NULL,
		updated_at INTEGER NOT NULL
	);
	CREATE TABLE media_cache (
		url_hash TEXT PRIMARY KEY,
		path TEXT NOT NULL,
		bytes INTEGER NOT NULL,
		last_used INTEGER NOT NULL
	);
	"];

pub fn apply(conn: &mut Connection) -> Result<()> {
	let current: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
	let supported = MIGRATIONS.len() as u32;
	if current > supported {
		return Err(StoreError::SchemaTooNew {
			found: current,
			supported,
		});
	}
	for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
		let tx = conn.transaction()?;
		tx.execute_batch(sql)?;
		tx.pragma_update(None, "user_version", index as u32 + 1)?;
		tx.commit()?;
	}
	Ok(())
}
