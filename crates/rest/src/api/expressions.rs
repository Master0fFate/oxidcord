use protocol::{Emoji, Gif, GifTrending, LenientVec, Snowflake, Sticker, StickerPack, StickerPacks};
use serde_json::{Map, Value, json};

use crate::client::RestClient;
use crate::error::Result;
use crate::request::{Request, UploadFile};

const GIF_PROVIDER: &str = "tenor";

fn gif_pairs(locale: &str, media_format: &str) -> [(&'static str, String); 3] {
	[
		("provider", GIF_PROVIDER.to_owned()),
		("locale", locale.to_owned()),
		("media_format", media_format.to_owned()),
	]
}

impl RestClient {
	pub async fn gifs_trending(&self, locale: &str, media_format: &str) -> Result<GifTrending> {
		self.send(Request::get("/gifs/trending").query(&gif_pairs(locale, media_format)))
			.await
	}

	pub async fn gifs_trending_gifs(
		&self,
		locale: &str,
		media_format: &str,
		limit: u16,
	) -> Result<LenientVec<Gif>> {
		let request = Request::get("/gifs/trending-gifs")
			.query(&gif_pairs(locale, media_format))
			.query(&[("limit", limit.to_string())]);
		self.send(request).await
	}

	pub async fn gifs_search(
		&self,
		query: &str,
		locale: &str,
		media_format: &str,
		limit: u16,
	) -> Result<LenientVec<Gif>> {
		let request = Request::get("/gifs/search")
			.query(&[("q", query.to_owned()), ("limit", limit.to_string())])
			.query(&gif_pairs(locale, media_format));
		self.send(request).await
	}

	pub async fn gifs_suggest(&self, query: &str, limit: u8) -> Result<Vec<String>> {
		let request =
			Request::get("/gifs/suggest").query(&[("q", query.to_owned()), ("limit", limit.to_string())]);
		self.send(request).await
	}

	pub async fn gifs_trending_search(&self, locale: &str) -> Result<Vec<String>> {
		let request =
			Request::get("/gifs/trending-search").query(&[("provider", GIF_PROVIDER), ("locale", locale)]);
		self.send(request).await
	}

	pub async fn gif_select(&self, id: &str, search_term: Option<&str>) -> Result<()> {
		let request = Request::post("/gifs/select").json(&json!({ "id": id, "q": search_term }));
		self.send_empty(request).await
	}

	pub async fn gif_favorites(&self) -> Result<LenientVec<Gif>> {
		self.send(Request::get("/users/@me/gifs/favorites")).await
	}

	pub async fn gif_favorite_add(&self, id: &str) -> Result<()> {
		self.send_empty(Request::post("/users/@me/gifs/favorites").json(&json!({ "id": id })))
			.await
	}

	pub async fn gif_favorite_remove(&self, id: &str) -> Result<()> {
		self.send_empty(Request::delete(format!("/users/@me/gifs/favorites/{id}")))
			.await
	}

	pub async fn sticker_packs(&self) -> Result<StickerPacks> {
		self.send(Request::get("/sticker-packs")).await
	}

	pub async fn sticker_pack(&self, id: Snowflake) -> Result<StickerPack> {
		self.send(Request::get(format!("/sticker-packs/{id}"))).await
	}

	pub async fn sticker(&self, id: Snowflake) -> Result<Sticker> {
		self.send(Request::get(format!("/stickers/{id}"))).await
	}

	pub async fn guild_stickers(&self, guild_id: Snowflake) -> Result<LenientVec<Sticker>> {
		self.send(Request::get(format!("/guilds/{guild_id}/stickers")))
			.await
	}

	pub async fn create_guild_sticker(
		&self,
		guild_id: Snowflake,
		name: &str,
		description: &str,
		tags: &str,
		file: UploadFile,
	) -> Result<Sticker> {
		let fields = vec![
			("name".to_owned(), name.to_owned()),
			("description".to_owned(), description.to_owned()),
			("tags".to_owned(), tags.to_owned()),
		];
		let request = Request::post(format!("/guilds/{guild_id}/stickers"))
			.multipart_fields(fields, vec![("file".to_owned(), file)]);
		self.send(request).await
	}

	pub async fn edit_guild_sticker(
		&self,
		guild_id: Snowflake,
		id: Snowflake,
		name: Option<&str>,
		description: Option<&str>,
		tags: Option<&str>,
	) -> Result<Sticker> {
		let mut changes = Map::new();
		for (key, value) in [("name", name), ("description", description), ("tags", tags)] {
			if let Some(value) = value {
				changes.insert(key.to_owned(), Value::from(value));
			}
		}
		let request = Request::patch(format!("/guilds/{guild_id}/stickers/{id}")).json(&changes);
		self.send(request).await
	}

	pub async fn delete_guild_sticker(&self, guild_id: Snowflake, id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/guilds/{guild_id}/stickers/{id}")))
			.await
	}

	pub async fn emoji(&self, guild_id: Snowflake, id: Snowflake) -> Result<Emoji> {
		self.send(Request::get(format!("/guilds/{guild_id}/emojis/{id}")))
			.await
	}

	pub async fn create_guild_emoji(
		&self,
		guild_id: Snowflake,
		name: &str,
		image_data_uri: &str,
		roles: &[Snowflake],
	) -> Result<Emoji> {
		let request = Request::post(format!("/guilds/{guild_id}/emojis"))
			.json(&json!({ "name": name, "image": image_data_uri, "roles": roles }));
		self.send(request).await
	}

	pub async fn edit_guild_emoji(
		&self,
		guild_id: Snowflake,
		id: Snowflake,
		name: Option<&str>,
		roles: Option<&[Snowflake]>,
	) -> Result<Emoji> {
		let mut changes = Map::new();
		if let Some(name) = name {
			changes.insert("name".to_owned(), Value::from(name));
		}
		if let Some(roles) = roles {
			changes.insert("roles".to_owned(), json!(roles));
		}
		let request = Request::patch(format!("/guilds/{guild_id}/emojis/{id}")).json(&changes);
		self.send(request).await
	}

	pub async fn delete_guild_emoji(&self, guild_id: Snowflake, id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/guilds/{guild_id}/emojis/{id}")))
			.await
	}
}
