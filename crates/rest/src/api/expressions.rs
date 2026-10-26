use protocol::{
	Emoji, Gif, GifFavorites, GifFormat, GifTrending, LenientVec, ProtoFavoriteGif, ProtoFrecencySettings,
	ProtoGifFavorites, Snowflake, Sticker, StickerPack, StickerPacks, decode_frecency_settings,
	encode_frecency_favorites,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::RestClient;
use crate::error::{RestError, Result};
use crate::request::{Request, UploadFile};

const GIF_PROVIDER: &str = "tenor";
const FRECENCY_SETTINGS: &str = "/users/@me/settings-proto/2";
const FRECENCY_ATTEMPTS: usize = 2;
const GIF_FORMAT_IMAGE: u32 = 1;
const GIF_FORMAT_VIDEO: u32 = 2;

#[derive(Deserialize, Default)]
#[serde(default)]
struct SettingsProtoPayload {
	settings: String,
	out_of_date: bool,
}

fn gif_pairs(locale: &str, media_format: &str) -> [(&'static str, String); 3] {
	[
		("provider", GIF_PROVIDER.to_owned()),
		("locale", locale.to_owned()),
		("media_format", media_format.to_owned()),
	]
}

fn favorite_gifs(settings: &ProtoFrecencySettings) -> GifFavorites {
	let mut ordered: Vec<&ProtoFavoriteGif> = settings.favorites.gifs.iter().collect();
	ordered.sort_by_key(|favorite| std::cmp::Reverse(favorite.order));
	let gifs = ordered
		.into_iter()
		.map(|favorite| Gif {
			id: favorite.url.clone(),
			title: String::new(),
			url: favorite.url.clone(),
			src: favorite.src.clone(),
			gif_src: favorite.src.clone(),
			width: favorite.width,
			height: favorite.height,
			preview: String::new(),
			format: match favorite.format {
				GIF_FORMAT_VIDEO => GifFormat::Mp4,
				GIF_FORMAT_IMAGE => GifFormat::Gif,
				_ => GifFormat::Unknown(String::new()),
			},
		})
		.collect();
	GifFavorites {
		gifs,
		version: settings.data_version,
	}
}

fn favorite_format(gif: &Gif) -> u32 {
	match gif.format {
		GifFormat::Mp4 => GIF_FORMAT_VIDEO,
		GifFormat::Gif => GIF_FORMAT_IMAGE,
		GifFormat::Unknown(_) if gif.gif_src.ends_with(".mp4") => GIF_FORMAT_VIDEO,
		GifFormat::Unknown(_) => GIF_FORMAT_IMAGE,
	}
}

fn add_favorite(favorites: &mut ProtoGifFavorites, gif: &Gif) {
	if gif.url.is_empty() || favorites.gifs.iter().any(|favorite| favorite.url == gif.url) {
		return;
	}
	let order = favorites
		.gifs
		.iter()
		.map(|favorite| favorite.order)
		.max()
		.map_or(0, |order| order.saturating_add(1));
	favorites.gifs.push(ProtoFavoriteGif {
		url: gif.url.clone(),
		format: favorite_format(gif),
		src: if gif.gif_src.is_empty() {
			gif.src.clone()
		} else {
			gif.gif_src.clone()
		},
		width: gif.width,
		height: gif.height,
		order,
	});
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

	pub async fn gif_favorites(&self) -> Result<GifFavorites> {
		let payload: SettingsProtoPayload = self.send(Request::get(FRECENCY_SETTINGS)).await?;
		let settings = decode_frecency_settings(&payload.settings).ok_or(RestError::InvalidSettings)?;
		Ok(favorite_gifs(&settings))
	}

	pub async fn favorite_gif(&self, gif: &Gif) -> Result<GifFavorites> {
		self.write_gif_favorites(Some(gif), None).await
	}

	pub async fn unfavorite_gif(&self, url: &str) -> Result<GifFavorites> {
		self.write_gif_favorites(None, Some(url)).await
	}

	async fn write_gif_favorites(&self, add: Option<&Gif>, remove: Option<&str>) -> Result<GifFavorites> {
		let _guard = self.frecency_guard().await;
		for _ in 0..FRECENCY_ATTEMPTS {
			let payload: SettingsProtoPayload = self.send(Request::get(FRECENCY_SETTINGS)).await?;
			let mut settings =
				decode_frecency_settings(&payload.settings).ok_or(RestError::InvalidSettings)?;
			if let Some(gif) = add {
				add_favorite(&mut settings.favorites, gif);
			}
			if let Some(url) = remove {
				settings.favorites.gifs.retain(|favorite| favorite.url != url);
			}
			let request = Request::patch(FRECENCY_SETTINGS).json(&json!({
				"settings": encode_frecency_favorites(&settings.favorites),
				"required_data_version": settings.data_version,
			}));
			let response: SettingsProtoPayload = self.send(request).await?;
			if response.out_of_date {
				continue;
			}
			let saved = decode_frecency_settings(&response.settings)
				.filter(|saved| saved.data_version > settings.data_version)
				.unwrap_or(settings);
			return Ok(favorite_gifs(&saved));
		}
		Err(RestError::OutOfDate)
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
