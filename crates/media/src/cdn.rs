use url::Url;

const CDN: &str = "https://cdn.discordapp.com";
const MEDIA_PROXY: &str = "https://media.discordapp.net";
const CDN_HOST: &str = "cdn.discordapp.com";
const PROXY_HOSTS: [&str; 3] = [
	"media.discordapp.net",
	"images-ext-1.discordapp.net",
	"images-ext-2.discordapp.net",
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
	let Ok(mut parsed) = Url::parse(url) else {
		return url.to_owned();
	};
	let host = parsed.host_str().unwrap_or_default().to_owned();
	let rewritable = if host == CDN_HOST {
		parsed.path().starts_with("/attachments/") && parsed.set_host(Some("media.discordapp.net")).is_ok()
	} else {
		PROXY_HOSTS.contains(&host.as_str())
	};
	if !rewritable {
		return url.to_owned();
	}
	let kept: Vec<(String, String)> = parsed
		.query_pairs()
		.filter(|(key, _)| !matches!(key.as_ref(), "width" | "height" | "format"))
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
	}
	parsed.into()
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
