use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use protocol::{
	LenientVec, Snowflake, Sticker, StickerFormat, StickerKind, StickerPack, decode_frecency_settings,
	encode_frecency_settings,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{ApiError, ApiResult};
use crate::ids::snowflake_at;
use crate::json::{parse_id, to_json};
use crate::state::Shared;

type Mock = State<Arc<Shared>>;

const GIF_NAMES: [&str; 20] = [
	"wave",
	"thumbs-up",
	"party",
	"facepalm",
	"mind-blown",
	"dance",
	"cat",
	"dog",
	"coffee",
	"fire",
	"sad",
	"laugh",
	"clap",
	"shrug",
	"thinking",
	"cheers",
	"celebrate",
	"sleepy",
	"heart",
	"popcorn",
];
const GIF_CATEGORIES: [&str; 8] = [
	"reactions",
	"celebrate",
	"animals",
	"memes",
	"love",
	"sad",
	"anime",
	"sports",
];
const TRENDING_TERMS: [&str; 10] = [
	"good morning",
	"thank you",
	"lol",
	"bye",
	"wow",
	"yes",
	"no",
	"hello",
	"congrats",
	"oops",
];
const SUGGESTION_SUFFIXES: [&str; 6] = ["", " reaction", " funny", " meme", " cat", " dance"];
const GIF_ID_BASE: u64 = 12_409_989_992_265_318_000;
const GIF_WIDTH: u32 = 400;
const GIF_HEIGHT: u32 = 300;
const DEFAULT_GIF_LIMIT: usize = 100;
const MAX_GIF_LIMIT: usize = 500;
const DEFAULT_SUGGEST_LIMIT: usize = 20;
const API_SUFFIX: &str = "/api/v10";

const PACKS: [(&str, &str); 3] = [
	("Mock Friends", "A few faces for testing the picker"),
	("Mock Reactions", "Quick reactions that never load a real asset"),
	("Mock Critters", "Small animals of the mock server"),
];
const PACK_STICKERS: [&str; 4] = ["hello", "thanks", "oops", "bye"];
const GUILD_STICKERS: [(&str, &str); 2] = [("crab_wave", "crab,wave"), ("ship_it", "ship,release")];
const PACK_EPOCH_MS: i64 = 1_641_000_000_000;
const PACK_STICKER_EPOCH_MS: i64 = 1_642_000_000_000;
const GUILD_STICKER_EPOCH_MS: i64 = 1_643_000_000_000;
const MIN_STICKER_NAME: usize = 2;
const MAX_STICKER_NAME: usize = 30;

type StickerStore = Mutex<HashMap<(usize, u64), Vec<Sticker>>>;

static GUILD_STICKER_STORE: LazyLock<StickerStore> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize, Default)]
pub struct GifParams {
	q: Option<String>,
	limit: Option<usize>,
}

pub fn routes() -> Router<Arc<Shared>> {
	Router::new()
		.route("/gifs/trending", get(gifs_trending))
		.route("/gifs/trending-gifs", get(gifs_trending_gifs))
		.route("/gifs/search", get(gifs_search))
		.route("/gifs/suggest", get(gifs_suggest))
		.route("/gifs/trending-search", get(gifs_trending_search))
		.route("/gifs/select", post(gif_select))
		.route(
			"/users/@me/settings-proto/2",
			get(frecency_settings).patch(patch_frecency_settings),
		)
		.route("/sticker-packs", get(sticker_packs))
		.route("/sticker-packs/{id}", get(sticker_pack))
		.route("/stickers/{id}", get(sticker))
		.route(
			"/guilds/{id}/stickers",
			get(guild_stickers).post(create_guild_sticker),
		)
		.route(
			"/guilds/{id}/stickers/{sid}",
			get(guild_sticker)
				.patch(edit_guild_sticker)
				.delete(delete_guild_sticker),
		)
		.route(
			"/guilds/{id}/emojis/{eid}",
			get(guild_emoji).patch(edit_guild_emoji),
		)
}

fn media_base(shared: &Shared) -> &str {
	shared.urls.rest.trim_end_matches(API_SUFFIX)
}

fn gif(base: &str, index: usize) -> Value {
	let name = GIF_NAMES[index];
	let id = (GIF_ID_BASE + index as u64).to_string();
	let src = format!("{base}/media/{name}.gif");
	json!({
		"id": id,
		"title": "",
		"url": format!("https://tenor.com/view/{name}-gif-{id}"),
		"gif_src": src,
		"src": src,
		"width": GIF_WIDTH,
		"height": GIF_HEIGHT,
		"preview": format!("{base}/media/{name}-preview.png"),
	})
}

fn gifs_matching(base: &str, query: &str, limit: Option<usize>) -> Vec<Value> {
	let needle = query.to_lowercase().replace(' ', "-");
	let mut indexes: Vec<usize> = (0..GIF_NAMES.len())
		.filter(|&index| GIF_NAMES[index].contains(&needle))
		.collect();
	if indexes.is_empty() {
		indexes = (0..GIF_NAMES.len()).collect();
	}
	indexes
		.into_iter()
		.take(limit.unwrap_or(DEFAULT_GIF_LIMIT).clamp(1, MAX_GIF_LIMIT))
		.map(|index| gif(base, index))
		.collect()
}

async fn gifs_trending(State(shared): Mock) -> ApiResult {
	let base = media_base(&shared);
	let categories: Vec<Value> = GIF_CATEGORIES
		.iter()
		.map(|name| json!({ "name": name, "src": format!("{base}/media/category-{name}.gif") }))
		.collect();
	let gifs = gifs_matching(base, "", Some(1));
	Ok(Json(json!({ "categories": categories, "gifs": gifs })))
}

async fn gifs_trending_gifs(State(shared): Mock, Query(params): Query<GifParams>) -> ApiResult {
	Ok(Json(Value::Array(gifs_matching(
		media_base(&shared),
		"",
		params.limit,
	))))
}

async fn gifs_search(State(shared): Mock, Query(params): Query<GifParams>) -> ApiResult {
	let query = params.q.unwrap_or_default();
	Ok(Json(Value::Array(gifs_matching(
		media_base(&shared),
		&query,
		params.limit,
	))))
}

async fn gifs_suggest(Query(params): Query<GifParams>) -> ApiResult {
	let query = params.q.unwrap_or_default();
	let terms: Vec<String> = SUGGESTION_SUFFIXES
		.iter()
		.take(params.limit.unwrap_or(DEFAULT_SUGGEST_LIMIT))
		.map(|suffix| format!("{query}{suffix}"))
		.collect();
	Ok(Json(to_json(&terms)))
}

async fn gifs_trending_search() -> ApiResult {
	Ok(Json(to_json(&TRENDING_TERMS)))
}

async fn gif_select(Json(_): Json<Value>) -> StatusCode {
	StatusCode::NO_CONTENT
}

async fn frecency_settings(State(shared): Mock) -> ApiResult {
	let settings = shared.store.lock().frecency.clone();
	Ok(Json(json!({
		"settings": encode_frecency_settings(&settings),
		"required_data_version": settings.data_version,
	})))
}

async fn patch_frecency_settings(
	State(shared): Mock,
	Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
	let encoded = body["settings"]
		.as_str()
		.ok_or_else(|| ApiError::invalid_body("settings is required"))?;
	let incoming =
		decode_frecency_settings(encoded).ok_or_else(|| ApiError::invalid_body("settings is not valid"))?;
	let required = body["required_data_version"].as_u64().unwrap_or_default();
	let mut store = shared.store.lock();
	if required != u64::from(store.frecency.data_version) {
		return Ok(Json(json!({ "out_of_date": true })));
	}
	store.frecency.favorites = incoming.favorites;
	store.frecency.data_version += 1;
	let settings = store.frecency.clone();
	Ok(Json(json!({
		"settings": encode_frecency_settings(&settings),
		"required_data_version": settings.data_version,
	})))
}

fn pack_sticker(pack_index: usize, index: usize, pack_id: Snowflake) -> Sticker {
	let offset = (pack_index * PACK_STICKERS.len() + index) as i64;
	Sticker {
		id: Snowflake(snowflake_at(PACK_STICKER_EPOCH_MS + offset * 1000, 0)),
		pack_id: Some(pack_id),
		name: PACK_STICKERS[index].to_owned(),
		description: Some(format!("{} says {}", PACKS[pack_index].0, PACK_STICKERS[index])),
		tags: Some(PACK_STICKERS[index].to_owned()),
		kind: Some(StickerKind::Standard),
		format_type: Some(StickerFormat::Png),
		available: Some(true),
		sort_value: Some(index as i64),
		..Sticker::default()
	}
}

fn packs() -> Vec<StickerPack> {
	PACKS
		.iter()
		.enumerate()
		.map(|(pack_index, (name, description))| {
			let id = Snowflake(snowflake_at(PACK_EPOCH_MS + pack_index as i64 * 1000, 0));
			let stickers: Vec<Sticker> = (0..PACK_STICKERS.len())
				.map(|index| pack_sticker(pack_index, index, id))
				.collect();
			StickerPack {
				id,
				name: (*name).to_owned(),
				sku_id: Snowflake(snowflake_at(PACK_EPOCH_MS + pack_index as i64 * 1000, 1)),
				cover_sticker_id: stickers.first().map(|sticker| sticker.id),
				description: Some((*description).to_owned()),
				banner_asset_id: None,
				stickers: LenientVec::from(stickers),
			}
		})
		.collect()
}

async fn sticker_packs() -> ApiResult {
	Ok(Json(json!({ "sticker_packs": to_json(&packs()) })))
}

async fn sticker_pack(Path(id): Path<u64>) -> ApiResult {
	packs()
		.into_iter()
		.find(|pack| pack.id.0 == id)
		.map(|pack| Json(to_json(&pack)))
		.ok_or_else(ApiError::not_found)
}

fn store_key(shared: &Arc<Shared>, guild_id: u64) -> (usize, u64) {
	(Arc::as_ptr(shared) as usize, guild_id)
}

fn live_guild_index(shared: &Shared, id: u64) -> Result<usize, ApiError> {
	let index = shared
		.fixtures
		.guilds
		.iter()
		.position(|guild| guild.id == id)
		.ok_or_else(ApiError::unknown_guild)?;
	if shared.store.lock().left_guilds.contains(&id) {
		return Err(ApiError::unknown_guild());
	}
	Ok(index)
}

fn seeded_stickers(shared: &Shared, guild_index: usize, guild_id: u64) -> Vec<Sticker> {
	let user = shared.store.lock().current_user.clone();
	GUILD_STICKERS
		.iter()
		.enumerate()
		.map(|(index, (name, tags))| {
			let offset = (guild_index * GUILD_STICKERS.len() + index) as i64;
			Sticker {
				id: Snowflake(snowflake_at(GUILD_STICKER_EPOCH_MS + offset * 1000, 0)),
				name: (*name).to_owned(),
				description: Some(String::new()),
				tags: Some((*tags).to_owned()),
				kind: Some(StickerKind::Guild),
				format_type: Some(StickerFormat::Png),
				available: Some(true),
				guild_id: Some(Snowflake(guild_id)),
				user: Some(user.clone()),
				..Sticker::default()
			}
		})
		.collect()
}

fn with_stickers<R>(
	shared: &Arc<Shared>,
	guild_id: u64,
	apply: impl FnOnce(&mut Vec<Sticker>) -> Result<R, ApiError>,
) -> Result<R, ApiError> {
	let index = live_guild_index(shared, guild_id)?;
	let seeded = seeded_stickers(shared, index, guild_id);
	let mut stores = GUILD_STICKER_STORE.lock();
	apply(stores.entry(store_key(shared, guild_id)).or_insert(seeded))
}

fn broadcast_stickers(shared: &Shared, guild_id: u64, stickers: &[Sticker]) {
	shared.hub.broadcast(
		"GUILD_STICKERS_UPDATE",
		&json!({ "guild_id": guild_id.to_string(), "stickers": to_json(stickers) }),
	);
}

async fn sticker(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	let standard = packs()
		.into_iter()
		.flat_map(|pack| pack.stickers.into_vec())
		.find(|sticker| sticker.id.0 == id);
	if let Some(sticker) = standard {
		return Ok(Json(to_json(&sticker)));
	}
	let guild_ids: Vec<u64> = shared.fixtures.guilds.iter().map(|guild| guild.id).collect();
	for guild_id in guild_ids {
		let found = with_stickers(&shared, guild_id, |stickers| {
			Ok(stickers.iter().find(|sticker| sticker.id.0 == id).cloned())
		});
		if let Ok(Some(sticker)) = found {
			return Ok(Json(to_json(&sticker)));
		}
	}
	Err(ApiError::not_found())
}

async fn guild_stickers(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	with_stickers(&shared, id, |stickers| Ok(Json(to_json(&*stickers))))
}

async fn guild_sticker(State(shared): Mock, Path((id, sticker_id)): Path<(u64, u64)>) -> ApiResult {
	with_stickers(&shared, id, |stickers| {
		stickers
			.iter()
			.find(|sticker| sticker.id.0 == sticker_id)
			.map(|sticker| Json(to_json(sticker)))
			.ok_or_else(ApiError::not_found)
	})
}

#[derive(Default)]
struct StickerForm {
	name: String,
	description: String,
	tags: String,
	format: Option<StickerFormat>,
}

fn multipart_error<E>(_: E) -> ApiError {
	ApiError::invalid_body("Invalid multipart body")
}

fn format_of_file(file_name: &str) -> StickerFormat {
	match file_name
		.rsplit_once('.')
		.map(|(_, extension)| extension.to_ascii_lowercase())
		.as_deref()
	{
		Some("gif") => StickerFormat::Gif,
		Some("json") => StickerFormat::Lottie,
		Some("apng") => StickerFormat::Apng,
		_ => StickerFormat::Png,
	}
}

fn apply_payload(form: &mut StickerForm, payload: &Value) {
	for (key, target) in [
		("name", &mut form.name),
		("description", &mut form.description),
		("tags", &mut form.tags),
	] {
		if let Some(value) = payload[key].as_str() {
			*target = value.to_owned();
		}
	}
}

async fn read_sticker_form(multipart: &mut Multipart) -> Result<StickerForm, ApiError> {
	let mut form = StickerForm::default();
	while let Some(field) = multipart.next_field().await.map_err(multipart_error)? {
		let field_name = field.name().unwrap_or_default().to_owned();
		if let Some(file_name) = field.file_name() {
			form.format = Some(format_of_file(file_name));
			field.bytes().await.map_err(multipart_error)?;
			continue;
		}
		let text = field.text().await.map_err(multipart_error)?;
		match field_name.as_str() {
			"payload_json" => {
				apply_payload(&mut form, &serde_json::from_str(&text).map_err(multipart_error)?)
			}
			"name" => form.name = text,
			"description" => form.description = text,
			"tags" => form.tags = text,
			_ => {}
		}
	}
	Ok(form)
}

async fn create_guild_sticker(
	State(shared): Mock,
	Path(id): Path<u64>,
	mut multipart: Multipart,
) -> ApiResult {
	live_guild_index(&shared, id)?;
	let form = read_sticker_form(&mut multipart).await?;
	let name_length = form.name.chars().count();
	if !(MIN_STICKER_NAME..=MAX_STICKER_NAME).contains(&name_length) {
		return Err(ApiError::invalid_body("name must be 2-30 characters"));
	}
	let format = form
		.format
		.ok_or_else(|| ApiError::invalid_body("file is required"))?;
	let (sticker_id, user) = {
		let mut store = shared.store.lock();
		(store.next_snowflake(), store.current_user.clone())
	};
	let sticker = Sticker {
		id: Snowflake(sticker_id),
		name: form.name,
		description: Some(form.description),
		tags: Some(form.tags),
		kind: Some(StickerKind::Guild),
		format_type: Some(format),
		available: Some(true),
		guild_id: Some(Snowflake(id)),
		user: Some(user),
		..Sticker::default()
	};
	let value = to_json(&sticker);
	with_stickers(&shared, id, |stickers| {
		stickers.push(sticker);
		broadcast_stickers(&shared, id, stickers);
		Ok(())
	})?;
	Ok(Json(value))
}

async fn edit_guild_sticker(
	State(shared): Mock,
	Path((id, sticker_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> ApiResult {
	with_stickers(&shared, id, |stickers| {
		let sticker = stickers
			.iter_mut()
			.find(|sticker| sticker.id.0 == sticker_id)
			.ok_or_else(ApiError::not_found)?;
		if let Some(name) = body["name"].as_str() {
			sticker.name = name.to_owned();
		}
		if let Some(description) = body["description"].as_str() {
			sticker.description = Some(description.to_owned());
		}
		if let Some(tags) = body["tags"].as_str() {
			sticker.tags = Some(tags.to_owned());
		}
		let value = to_json(&*sticker);
		broadcast_stickers(&shared, id, stickers);
		Ok(Json(value))
	})
}

async fn delete_guild_sticker(
	State(shared): Mock,
	Path((id, sticker_id)): Path<(u64, u64)>,
) -> Result<StatusCode, ApiError> {
	with_stickers(&shared, id, |stickers| {
		let before = stickers.len();
		stickers.retain(|sticker| sticker.id.0 != sticker_id);
		if stickers.len() == before {
			return Err(ApiError::not_found());
		}
		broadcast_stickers(&shared, id, stickers);
		Ok(StatusCode::NO_CONTENT)
	})
}

async fn guild_emoji(State(shared): Mock, Path((id, emoji_id)): Path<(u64, u64)>) -> ApiResult {
	live_guild_index(&shared, id)?;
	let store = shared.store.lock();
	store
		.emojis
		.get(&id)
		.and_then(|emojis| {
			emojis
				.iter()
				.find(|emoji| emoji.id.map(Snowflake::get) == Some(emoji_id))
		})
		.map(|emoji| Json(to_json(emoji)))
		.ok_or_else(ApiError::not_found)
}

async fn edit_guild_emoji(
	State(shared): Mock,
	Path((id, emoji_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> ApiResult {
	live_guild_index(&shared, id)?;
	let mut store = shared.store.lock();
	let emojis = store.emojis.get_mut(&id).ok_or_else(ApiError::not_found)?;
	let emoji = emojis
		.iter_mut()
		.find(|emoji| emoji.id.map(Snowflake::get) == Some(emoji_id))
		.ok_or_else(ApiError::not_found)?;
	if let Some(name) = body["name"].as_str() {
		emoji.name = Some(name.to_owned());
	}
	match &body["roles"] {
		Value::Array(roles) => emoji.roles = roles.iter().filter_map(parse_id).map(Snowflake).collect(),
		Value::Null if body.get("roles").is_some() => emoji.roles.clear(),
		_ => {}
	}
	let value = to_json(&*emoji);
	let all = to_json(&*emojis);
	drop(store);
	shared.hub.broadcast(
		"GUILD_EMOJIS_UPDATE",
		&json!({ "guild_id": id.to_string(), "emojis": all }),
	);
	Ok(Json(value))
}
