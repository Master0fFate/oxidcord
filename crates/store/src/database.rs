use std::path::Path;
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Result, StoreError};
use crate::migrations;
use crate::seal::{Sealer, is_sealed};

const CANARY_KEY: &str = "seal-check";
const CANARY_VALUE: &[u8] = b"oxidcord";
const SEALED_COLUMNS: [(&str, &str, &str); 6] = [
	("kv", "value", "key"),
	("guilds", "json", "id"),
	("channels", "json", "id"),
	("users", "json", "id"),
	("messages", "json", "id"),
	("drafts", "content", "channel_id"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
	Guilds,
	Channels,
	Users,
	Messages,
}

impl Table {
	fn upsert_sql(self) -> &'static str {
		match self {
			Self::Guilds => {
				"INSERT INTO guilds (id, json, updated_at) VALUES (?1, ?3, ?4)
				ON CONFLICT (id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at"
			}
			Self::Channels => {
				"INSERT INTO channels (id, guild_id, json, updated_at) VALUES (?1, ?2, ?3, ?4)
				ON CONFLICT (id) DO UPDATE SET guild_id = excluded.guild_id, json = excluded.json,
					updated_at = excluded.updated_at"
			}
			Self::Users => {
				"INSERT INTO users (id, json, updated_at) VALUES (?1, ?3, ?4)
				ON CONFLICT (id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at"
			}
			Self::Messages => {
				"INSERT INTO messages (id, channel_id, json, updated_at) VALUES (?1, ?2, ?3, ?4)
				ON CONFLICT (id) DO UPDATE SET channel_id = excluded.channel_id, json = excluded.json,
					updated_at = excluded.updated_at"
			}
		}
	}

	fn select_sql(self) -> &'static str {
		match self {
			Self::Guilds => "SELECT json FROM guilds WHERE id = ?1",
			Self::Channels => "SELECT json FROM channels WHERE id = ?1",
			Self::Users => "SELECT json FROM users WHERE id = ?1",
			Self::Messages => "SELECT json FROM messages WHERE id = ?1",
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadState {
	pub channel_id: u64,
	pub last_message_id: Option<u64>,
	pub mention_count: u32,
	pub last_viewed: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaEntry {
	pub path: String,
	pub bytes: u64,
	pub last_used: i64,
}

#[derive(Debug)]
pub struct Database {
	conn: Mutex<Connection>,
	sealer: Sealer,
}

fn now_millis() -> i64 {
	chrono::Utc::now().timestamp_millis()
}

impl Database {
	pub fn open(path: &Path, key: &[u8; 32]) -> Result<Self> {
		Self::configure(Connection::open(path)?, Sealer::new(key))
	}

	fn configure(mut conn: Connection, sealer: Sealer) -> Result<Self> {
		conn.busy_timeout(Duration::from_secs(5))?;
		conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
		conn.pragma_update(None, "synchronous", "NORMAL")?;
		conn.pragma_update(None, "foreign_keys", true)?;
		migrations::apply(&mut conn)?;
		prepare_sealed_columns(&mut conn, &sealer)?;
		Ok(Self {
			conn: Mutex::new(conn),
			sealer,
		})
	}

	pub fn put_json(&self, table: Table, id: u64, parent_id: Option<u64>, json: &[u8]) -> Result<()> {
		let json = self.sealer.seal(json)?;
		self.conn
			.lock()
			.prepare_cached(table.upsert_sql())?
			.execute(params![
				id as i64,
				parent_id.map(|parent| parent as i64),
				json,
				now_millis()
			])?;
		Ok(())
	}

	pub fn get_json(&self, table: Table, id: u64) -> Result<Option<Vec<u8>>> {
		let sealed: Option<Vec<u8>> = self
			.conn
			.lock()
			.prepare_cached(table.select_sql())?
			.query_row([id as i64], |row| row.get(0))
			.optional()?;
		sealed.map(|sealed| self.sealer.open(&sealed)).transpose()
	}

	pub fn recent_messages(&self, channel_id: u64, limit: u32) -> Result<Vec<(u64, Vec<u8>)>> {
		let conn = self.conn.lock();
		let mut statement = conn.prepare_cached(
			"SELECT id, json FROM messages WHERE channel_id = ?1 ORDER BY id DESC LIMIT ?2",
		)?;
		let rows = statement
			.query_map(params![channel_id as i64, limit], |row| {
				Ok((row.get::<_, i64>(0)? as u64, row.get::<_, Vec<u8>>(1)?))
			})?
			.collect::<rusqlite::Result<Vec<_>>>()?;
		rows.into_iter()
			.map(|(id, sealed)| Ok((id, self.sealer.open(&sealed)?)))
			.collect()
	}

	pub fn put_messages_batch(&self, channel_id: u64, messages: &[(u64, &[u8])]) -> Result<()> {
		let sealed = messages
			.iter()
			.map(|(id, json)| Ok((*id, self.sealer.seal(json)?)))
			.collect::<Result<Vec<_>>>()?;
		let mut conn = self.conn.lock();
		let tx = conn.transaction()?;
		{
			let mut statement = tx.prepare_cached(Table::Messages.upsert_sql())?;
			let now = now_millis();
			for (id, json) in &sealed {
				statement.execute(params![*id as i64, channel_id as i64, json, now])?;
			}
		}
		tx.commit()?;
		Ok(())
	}

	pub fn delete_message(&self, id: u64) -> Result<()> {
		self.conn
			.lock()
			.prepare_cached("DELETE FROM messages WHERE id = ?1")?
			.execute([id as i64])?;
		Ok(())
	}

	pub fn set_draft(&self, channel_id: u64, content: &str) -> Result<()> {
		let content = self.sealer.seal(content.as_bytes())?;
		self.conn
			.lock()
			.prepare_cached(
				"INSERT INTO drafts (channel_id, content, updated_at) VALUES (?1, ?2, ?3)
				ON CONFLICT (channel_id) DO UPDATE SET content = excluded.content,
					updated_at = excluded.updated_at",
			)?
			.execute(params![channel_id as i64, content, now_millis()])?;
		Ok(())
	}

	pub fn get_draft(&self, channel_id: u64) -> Result<Option<String>> {
		let sealed: Option<Vec<u8>> = self
			.conn
			.lock()
			.prepare_cached("SELECT content FROM drafts WHERE channel_id = ?1")?
			.query_row([channel_id as i64], |row| row.get(0))
			.optional()?;
		sealed
			.map(|sealed| {
				String::from_utf8(self.sealer.open(&sealed)?)
					.map_err(|_| StoreError::Seal("draft is not valid utf-8"))
			})
			.transpose()
	}

	pub fn clear_draft(&self, channel_id: u64) -> Result<()> {
		self.conn
			.lock()
			.prepare_cached("DELETE FROM drafts WHERE channel_id = ?1")?
			.execute([channel_id as i64])?;
		Ok(())
	}

	pub fn set_read_state(&self, state: &ReadState) -> Result<()> {
		self.conn
			.lock()
			.prepare_cached(
				"INSERT INTO read_state (channel_id, last_message_id, mention_count, last_viewed)
				VALUES (?1, ?2, ?3, ?4)
				ON CONFLICT (channel_id) DO UPDATE SET last_message_id = excluded.last_message_id,
					mention_count = excluded.mention_count, last_viewed = excluded.last_viewed",
			)?
			.execute(params![
				state.channel_id as i64,
				state.last_message_id.map(|id| id as i64),
				state.mention_count,
				state.last_viewed
			])?;
		Ok(())
	}

	pub fn get_read_state(&self, channel_id: u64) -> Result<Option<ReadState>> {
		Ok(self
			.conn
			.lock()
			.prepare_cached(
				"SELECT channel_id, last_message_id, mention_count, last_viewed FROM read_state
				WHERE channel_id = ?1",
			)?
			.query_row([channel_id as i64], read_state_from_row)
			.optional()?)
	}

	pub fn all_read_state(&self) -> Result<Vec<ReadState>> {
		let conn = self.conn.lock();
		let mut statement = conn.prepare_cached(
			"SELECT channel_id, last_message_id, mention_count, last_viewed FROM read_state",
		)?;
		let rows = statement
			.query_map([], read_state_from_row)?
			.collect::<rusqlite::Result<_>>()?;
		Ok(rows)
	}

	pub fn kv_get(&self, key: &str) -> Result<Option<Vec<u8>>> {
		let sealed: Option<Vec<u8>> = self
			.conn
			.lock()
			.prepare_cached("SELECT value FROM kv WHERE key = ?1")?
			.query_row([key], |row| row.get(0))
			.optional()?;
		sealed.map(|sealed| self.sealer.open(&sealed)).transpose()
	}

	pub fn kv_set(&self, key: &str, value: &[u8]) -> Result<()> {
		let value = self.sealer.seal(value)?;
		self.conn
			.lock()
			.prepare_cached(
				"INSERT INTO kv (key, value, updated_at) VALUES (?1, ?2, ?3)
				ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
			)?
			.execute(params![key, value, now_millis()])?;
		Ok(())
	}

	pub fn media_touch(&self, url_hash: &str) -> Result<()> {
		self.conn
			.lock()
			.prepare_cached("UPDATE media_cache SET last_used = ?2 WHERE url_hash = ?1")?
			.execute(params![url_hash, now_millis()])?;
		Ok(())
	}

	pub fn media_lookup(&self, url_hash: &str) -> Result<Option<MediaEntry>> {
		Ok(self
			.conn
			.lock()
			.prepare_cached("SELECT path, bytes, last_used FROM media_cache WHERE url_hash = ?1")?
			.query_row([url_hash], |row| {
				Ok(MediaEntry {
					path: row.get(0)?,
					bytes: row.get::<_, i64>(1)? as u64,
					last_used: row.get(2)?,
				})
			})
			.optional()?)
	}

	pub fn media_insert(&self, url_hash: &str, path: &str, bytes: u64) -> Result<()> {
		self.conn
			.lock()
			.prepare_cached(
				"INSERT INTO media_cache (url_hash, path, bytes, last_used) VALUES (?1, ?2, ?3, ?4)
				ON CONFLICT (url_hash) DO UPDATE SET path = excluded.path, bytes = excluded.bytes,
					last_used = excluded.last_used",
			)?
			.execute(params![url_hash, path, bytes as i64, now_millis()])?;
		Ok(())
	}

	pub fn media_evict_to(&self, limit_bytes: u64) -> Result<Vec<String>> {
		let mut conn = self.conn.lock();
		let tx = conn.transaction()?;
		let mut total: i64 = tx.query_row("SELECT COALESCE(SUM(bytes), 0) FROM media_cache", [], |row| {
			row.get(0)
		})?;
		let limit = limit_bytes as i64;
		let mut evicted_hashes = Vec::new();
		let mut evicted_paths = Vec::new();
		if total > limit {
			let mut statement = tx.prepare_cached(
				"SELECT url_hash, path, bytes FROM media_cache ORDER BY last_used ASC, rowid ASC",
			)?;
			let mut rows = statement.query([])?;
			while total > limit {
				let Some(row) = rows.next()? else { break };
				evicted_hashes.push(row.get::<_, String>(0)?);
				evicted_paths.push(row.get::<_, String>(1)?);
				total -= row.get::<_, i64>(2)?;
			}
		}
		{
			let mut delete = tx.prepare_cached("DELETE FROM media_cache WHERE url_hash = ?1")?;
			for url_hash in &evicted_hashes {
				delete.execute([url_hash])?;
			}
		}
		tx.commit()?;
		Ok(evicted_paths)
	}

	pub fn vacuum_if_over(&self, limit_bytes: u64) -> Result<bool> {
		let conn = self.conn.lock();
		let page_size: i64 = conn.pragma_query_value(None, "page_size", |row| row.get(0))?;
		let page_count: i64 = conn.pragma_query_value(None, "page_count", |row| row.get(0))?;
		if (page_size * page_count) as u64 <= limit_bytes {
			return Ok(false);
		}
		conn.execute_batch("VACUUM")?;
		Ok(true)
	}
}

fn read_state_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReadState> {
	Ok(ReadState {
		channel_id: row.get::<_, i64>(0)? as u64,
		last_message_id: row.get::<_, Option<i64>>(1)?.map(|id| id as u64),
		mention_count: row.get(2)?,
		last_viewed: row.get(3)?,
	})
}

fn prepare_sealed_columns(conn: &mut Connection, sealer: &Sealer) -> Result<()> {
	let canary: Option<Vec<u8>> = conn
		.query_row("SELECT value FROM kv WHERE key = ?1", [CANARY_KEY], |row| {
			row.get(0)
		})
		.optional()?;
	let canary_matches = canary
		.as_deref()
		.is_some_and(|sealed| sealer.open(sealed).is_ok_and(|plain| plain == CANARY_VALUE));
	if canary_matches {
		return Ok(());
	}
	let tx = conn.transaction()?;
	if canary.is_some() {
		tracing::warn!("cache key changed, dropping the encrypted cache");
		for (table, _, _) in SEALED_COLUMNS {
			tx.execute(&format!("DELETE FROM {table}"), [])?;
		}
	} else {
		for (table, column, primary_key) in SEALED_COLUMNS {
			seal_legacy_rows(&tx, sealer, table, column, primary_key)?;
		}
	}
	tx.execute(
		"INSERT INTO kv (key, value, updated_at) VALUES (?1, ?2, ?3)
		ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
		params![CANARY_KEY, sealer.seal(CANARY_VALUE)?, now_millis()],
	)?;
	tx.commit()?;
	conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM")?;
	Ok(())
}

fn seal_legacy_rows(
	tx: &rusqlite::Transaction<'_>,
	sealer: &Sealer,
	table: &str,
	column: &str,
	primary_key: &str,
) -> Result<()> {
	let rows = tx
		.prepare(&format!(
			"SELECT {primary_key}, CAST({column} AS BLOB) FROM {table}"
		))?
		.query_map([], |row| {
			Ok((
				row.get::<_, rusqlite::types::Value>(0)?,
				row.get::<_, Vec<u8>>(1)?,
			))
		})?
		.collect::<rusqlite::Result<Vec<_>>>()?;
	let mut update = tx.prepare(&format!(
		"UPDATE {table} SET {column} = ?1 WHERE {primary_key} = ?2"
	))?;
	for (id, plain) in rows.into_iter().filter(|(_, value)| !is_sealed(value)) {
		update.execute(params![sealer.seal(&plain)?, id])?;
	}
	Ok(())
}
