use url::Url;

const CDN: &str = "https://cdn.discordapp.com";
const MEDIA_PROXY: &str = "https://media.discordapp.net";
const CDN_HOST: &str = "cdn.discordapp.com";
const PROXY_HOSTS: [&str; 3] = [
	"media.discordapp.net",
	"images-ext-1.discordapp.net",
	"images-ext-2.discordapp.net",
];
const TENOR_HOST: &str = "media.tenor.com";
const TENOR_PREVIEW: &str = "AAAAe";
const TENOR_TINY_GIF: &str = "AAAAM";
const TENOR_CODE_LEN: usize = 5;
const VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "m4v", "webm", "mov", "mkv"];
const PLAYABLE_VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "m4v", "webm", "mov"];
const PLAYABLE_VIDEO_TYPES: [&str; 3] = ["video/mp4", "video/webm", "video/quicktime"];
const AUDIO_EXTENSIONS: [&str; 8] = ["mp3", "ogg", "oga", "opus", "wav", "flac", "m4a", "aac"];
const AUDIO_TYPES: [&str; 12] = [
	"audio/mpeg",
	"audio/mp3",
	"audio/ogg",
	"audio/opus",
	"audio/wav",
	"audio/x-wav",
	"audio/wave",
	"audio/flac",
	"audio/x-flac",
	"audio/mp4",
	"audio/x-m4a",
	"audio/aac",
];
const MIN_SIZE: u32 = 16;
const MAX_SIZE: u32 = 4096;
const STICKER_SIZE: u32 = 320;
const DEFAULT_AVATAR_BUCKETS: u64 = 6;
const LEGACY_DEFAULT_AVATAR_BUCKETS: u32 = 5;
const SNOWFLAKE_TIMESTAMP_SHIFT: u32 = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickerFormat {
	Png,
	Apng,
	Lottie,
	Gif,
	Unknown(i64),
}

impl From<i64> for StickerFormat {
	fn from(value: i64) -> Self {
		match value {
			1 => Self::Png,
			2 => Self::Apng,
			3 => Self::Lottie,
			4 => Self::Gif,
			other => Self::Unknown(other),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyFormat {
	Webp,
	Png,
	Jpeg,
}

impl ProxyFormat {
	fn as_str(self) -> &'static str {
		match self {
			Self::Webp => "webp",
			Self::Png => "png",
			Self::Jpeg => "jpeg",
		}
	}
}

pub fn clamp_size(size: u32) -> u32 {
	size.clamp(MIN_SIZE, MAX_SIZE).next_power_of_two()
}

fn is_animated(hash: &str) -> bool {
	hash.starts_with("a_")
}

fn extension(hash: &str) -> &'static str {
	if is_animated(hash) { "gif" } else { "webp" }
}

pub fn avatar_url(
	user_id: u64,
	avatar_hash: Option<&str>,
	legacy_discriminator: Option<&str>,
	size: u32,
) -> String {
	match avatar_hash {
		Some(hash) => format!(
			"{CDN}/avatars/{user_id}/{hash}.{}?size={}",
			extension(hash),
			clamp_size(size)
		),
		None => default_avatar_url(user_id, legacy_discriminator),
	}
}

pub fn default_avatar_url(user_id: u64, legacy_discriminator: Option<&str>) -> String {
	let index = match legacy_discriminator.and_then(|value| value.parse::<u32>().ok()) {
		Some(discriminator) if discriminator != 0 => u64::from(discriminator % LEGACY_DEFAULT_AVATAR_BUCKETS),
		_ => (user_id >> SNOWFLAKE_TIMESTAMP_SHIFT) % DEFAULT_AVATAR_BUCKETS,
	};
	format!("{CDN}/embed/avatars/{index}.png")
}

pub fn guild_icon_url(guild_id: u64, hash: &str, size: u32) -> String {
	format!(
		"{CDN}/icons/{guild_id}/{hash}.{}?size={}",
		extension(hash),
		clamp_size(size)
	)
}

pub fn emoji_url(id: u64, animated: bool, size: u32) -> String {
	let extension = if animated { "gif" } else { "webp" };
	format!(
		"{CDN}/emojis/{id}.{extension}?size={}&quality=lossless",
		clamp_size(size)
	)
}

pub fn sticker_url(id: u64, format: StickerFormat) -> Option<String> {
	match format {
		StickerFormat::Png | StickerFormat::Apng => {
			Some(format!("{CDN}/stickers/{id}.png?size={STICKER_SIZE}"))
		}
		StickerFormat::Gif => Some(format!("{MEDIA_PROXY}/stickers/{id}.gif?size={STICKER_SIZE}")),
		StickerFormat::Lottie | StickerFormat::Unknown(_) => None,
	}
}

pub fn banner_url(owner_id: u64, hash: &str, size: u32) -> String {
	format!(
		"{CDN}/banners/{owner_id}/{hash}.{}?size={}",
		extension(hash),
		clamp_size(size)
	)
}

pub fn role_icon_url(role_id: u64, hash: &str, size: u32) -> String {
	format!("{CDN}/role-icons/{role_id}/{hash}.webp?size={}", clamp_size(size))
}

pub fn application_icon_url(application_id: u64, hash: &str, size: u32) -> String {
	format!(
		"{CDN}/app-icons/{application_id}/{hash}.webp?size={}",
		clamp_size(size)
	)
}

pub fn activity_asset_url(application_id: Option<u64>, asset: &str, size: u32) -> Option<String> {
	if let Some(path) = asset.strip_prefix("mp:") {
		return (!path.contains("..")).then(|| format!("{MEDIA_PROXY}/{path}"));
	}
	if let Some(image) = asset.strip_prefix("spotify:") {
		return image
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric())
			.then(|| format!("https://i.scdn.co/image/{image}"));
	}
	if !asset.bytes().all(|byte| byte.is_ascii_digit()) {
		return None;
	}
	application_id.map(|application_id| {
		format!(
			"{CDN}/app-assets/{application_id}/{asset}.webp?size={}",
			clamp_size(size)
		)
	})
}

pub fn avatar_decoration_url(asset: &str, size: u32) -> String {
	let passthrough = if is_animated(asset) {
		"&passthrough=true"
	} else {
		""
	};
	format!(
		"{CDN}/avatar-decoration-presets/{asset}.png?size={}{passthrough}",
		clamp_size(size)
	)
}

pub fn attachment_proxy(url: &str, width: u32, height: u32, format: ProxyFormat) -> String {
	proxy_rewrite(url, width, height, format, None).unwrap_or_else(|| url.to_owned())
}

pub fn animated_proxy(url: &str, width: u32, height: u32) -> String {
	proxy_rewrite(url, width, height, ProxyFormat::Webp, Some(true)).unwrap_or_else(|| url.to_owned())
}

pub fn video_poster(url: &str, width: u32, height: u32) -> Option<String> {
	proxy_rewrite(url, width, height, ProxyFormat::Webp, None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playable {
	Video,
	Audio,
}

fn extension_of(name: &str) -> Option<&str> {
	let path = name.split(['?', '#']).next().unwrap_or_default();
	let file = path.rsplit('/').next().unwrap_or(path);
	file.rsplit_once('.').map(|(_, extension)| extension)
}

pub fn playable(content_type: Option<&str>, name: &str) -> Option<Playable> {
	let essence = content_type.map(|kind| {
		kind.split(';')
			.next()
			.unwrap_or_default()
			.trim()
			.to_ascii_lowercase()
	});
	match essence.as_deref() {
		Some(kind) if PLAYABLE_VIDEO_TYPES.contains(&kind) => return Some(Playable::Video),
		Some(kind) if AUDIO_TYPES.contains(&kind) => return Some(Playable::Audio),
		Some(kind) if !kind.is_empty() && kind != "application/octet-stream" => return None,
		_ => {}
	}
	let extension = extension_of(name)?;
	let matches = |known: &[&str]| known.iter().any(|item| extension.eq_ignore_ascii_case(item));
	if matches(&PLAYABLE_VIDEO_EXTENSIONS) {
		Some(Playable::Video)
	} else if matches(&AUDIO_EXTENSIONS) {
		Some(Playable::Audio)
	} else {
		None
	}
}

pub fn is_gif(content_type: Option<&str>, name: &str) -> bool {
	content_type.map_or_else(
		|| extension_of(name).is_some_and(|extension| extension.eq_ignore_ascii_case("gif")),
		|kind| kind.eq_ignore_ascii_case("image/gif"),
	)
}

fn proxy_rewrite(
	url: &str,
	width: u32,
	height: u32,
	format: ProxyFormat,
	animated: Option<bool>,
) -> Option<String> {
	let mut parsed = Url::parse(url).ok()?;
	let host = parsed.host_str().unwrap_or_default().to_owned();
	let rewritable = if host == CDN_HOST {
		parsed.path().starts_with("/attachments/") && parsed.set_host(Some("media.discordapp.net")).is_ok()
	} else {
		PROXY_HOSTS.contains(&host.as_str())
	};
	if !rewritable {
		return None;
	}
	let kept: Vec<(String, String)> = parsed
		.query_pairs()
		.filter(|(key, _)| {
			!matches!(key.as_ref(), "width" | "height" | "format")
				&& !(animated.is_some() && key == "animated")
		})
		.map(|(key, value)| (key.into_owned(), value.into_owned()))
		.collect();
	{
		let mut query = parsed.query_pairs_mut();
		query
			.clear()
			.extend_pairs(kept)
			.append_pair("format", format.as_str());
		if width > 0 {
			query.append_pair("width", &width.to_string());
		}
		if height > 0 {
			query.append_pair("height", &height.to_string());
		}
		if animated == Some(true) {
			query.append_pair("animated", "true");
		}
	}
	Some(parsed.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TenorRendition {
	Preview,
	TinyGif,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GifTile {
	pub still: String,
	pub animated: Option<String>,
}

pub fn is_video(url: &str) -> bool {
	let url = url.split('#').next().unwrap_or_default();
	let (path, query) = url.split_once('?').unwrap_or((url, ""));
	let Some((_, extension)) = path.rsplit_once('.') else {
		return false;
	};
	VIDEO_EXTENSIONS
		.iter()
		.any(|known| extension.eq_ignore_ascii_case(known))
		&& !query.split('&').any(|pair| pair.starts_with("format="))
}

pub fn gif_tile(source: &str, preview: &str, width: u32, height: u32) -> GifTile {
	if let Some(still) = tenor_rendition(source, TenorRendition::Preview) {
		return GifTile {
			still,
			animated: tenor_rendition(source, TenorRendition::TinyGif),
		};
	}
	if let Some(still) = proxy_rewrite(source, width, height, ProxyFormat::Webp, Some(false)) {
		return GifTile {
			still,
			animated: (!is_video(source))
				.then(|| proxy_rewrite(source, width, height, ProxyFormat::Webp, Some(true)))
				.flatten(),
		};
	}
	GifTile {
		still: if preview.is_empty() { source } else { preview }.to_owned(),
		animated: (!source.is_empty() && !is_video(source)).then(|| source.to_owned()),
	}
}

fn tenor_path(url: &str) -> Option<String> {
	let parsed = Url::parse(url).ok()?;
	let host = parsed.host_str()?;
	if host == TENOR_HOST {
		return Some(parsed.path().to_owned());
	}
	if !PROXY_HOSTS.contains(&host) {
		return None;
	}
	let proxied = parsed.path().strip_prefix("/external/")?;
	let (_, target) = proxied.split_once('/')?;
	target.strip_prefix("https/media.tenor.com").map(str::to_owned)
}

fn tenor_rendition(url: &str, rendition: TenorRendition) -> Option<String> {
	let path = tenor_path(url)?;
	let (asset, file) = path.trim_start_matches('/').split_once('/')?;
	let id = asset.get(..asset.len().checked_sub(TENOR_CODE_LEN)?)?;
	let (slug, _) = file.rsplit_once('.')?;
	if id.is_empty() || slug.is_empty() {
		return None;
	}
	let (code, extension) = match rendition {
		TenorRendition::Preview => (TENOR_PREVIEW, "png"),
		TenorRendition::TinyGif => (TENOR_TINY_GIF, "gif"),
	};
	Some(format!("https://{TENOR_HOST}/{id}{code}/{slug}.{extension}"))
}

pub fn guild_member_avatar_url(guild_id: u64, user_id: u64, hash: &str, size: u32) -> String {
	format!(
		"{CDN}/guilds/{guild_id}/users/{user_id}/avatars/{hash}.{}?size={}",
		extension(hash),
		clamp_size(size)
	)
}

pub fn badge_icon_url(icon: &str) -> String {
	format!("{CDN}/badge-icons/{icon}.png")
}
