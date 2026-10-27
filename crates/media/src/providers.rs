use url::Url;
use url::form_urlencoded::byte_serialize;

const YOUTUBE_ID_LEN: usize = 11;
const SPOTIFY_ID_LEN: usize = 22;
const MAX_SEGMENT_LEN: usize = 128;
const MAX_START_SECONDS: u64 = 24 * 3600;
const DEFAULT_VIDEO_WIDTH: u32 = 1280;
const DEFAULT_VIDEO_HEIGHT: u32 = 720;
const SPOTIFY_WIDTH: u32 = 400;
const SPOTIFY_TRACK_HEIGHT: u32 = 80;
const SPOTIFY_EPISODE_HEIGHT: u32 = 152;
const SPOTIFY_LIST_HEIGHT: u32 = 352;
const SOUNDCLOUD_WIDTH: u32 = 400;
const SOUNDCLOUD_TRACK_HEIGHT: u32 = 166;
const SOUNDCLOUD_SET_HEIGHT: u32 = 400;

pub const FRAME_ORIGINS: [&str; 6] = [
	"https://www.youtube.com",
	"https://consent.youtube.com",
	"https://player.vimeo.com",
	"https://open.spotify.com",
	"https://w.soundcloud.com",
	"https://www.youtube-nocookie.com",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
	YouTube,
	Vimeo,
	Spotify,
	SoundCloud,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
	Video,
	Widget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
	pub provider: Provider,
	pub layout: Layout,
	pub src: String,
	pub width: u32,
	pub height: u32,
}

pub struct Candidate<'a> {
	pub url: Option<&'a str>,
	pub video_url: Option<&'a str>,
	pub video_size: Option<(u32, u32)>,
}

pub fn player(candidate: &Candidate) -> Option<Player> {
	[candidate.video_url, candidate.url]
		.into_iter()
		.flatten()
		.filter_map(|raw| Url::parse(raw).ok())
		.filter(|url| url.scheme() == "https" && url.username().is_empty() && url.password().is_none())
		.find_map(|url| resolve(&url, candidate.video_size))
}

fn resolve(url: &Url, video_size: Option<(u32, u32)>) -> Option<Player> {
	let host = url.host_str()?.to_ascii_lowercase();
	let host = host.strip_prefix("www.").unwrap_or(&host);
	let (width, height) = video_size
		.filter(|(width, height)| *width > 0 && *height > 0)
		.unwrap_or((DEFAULT_VIDEO_WIDTH, DEFAULT_VIDEO_HEIGHT));
	match host {
		"youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be" | "youtube-nocookie.com" => {
			youtube(url, host).map(|(id, start)| {
				let mut src = format!("https://www.youtube.com/embed/{id}?autoplay=1&auto_play=1");
				if let Some(start) = start {
					src.push_str(&format!("&start={start}"));
				}
				video(Provider::YouTube, src, width, height)
			})
		}
		"vimeo.com" | "player.vimeo.com" => vimeo(url).map(|id| {
			video(
				Provider::Vimeo,
				format!("https://player.vimeo.com/video/{id}?autoplay=1"),
				width,
				height,
			)
		}),
		"open.spotify.com" => spotify(url),
		"soundcloud.com" | "m.soundcloud.com" => soundcloud(url),
		_ => None,
	}
}

fn video(provider: Provider, src: String, width: u32, height: u32) -> Player {
	Player {
		provider,
		layout: Layout::Video,
		src,
		width,
		height,
	}
}

fn segments(url: &Url) -> Vec<&str> {
	url.path_segments()
		.map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
		.unwrap_or_default()
}

fn query<'a>(url: &'a Url, key: &str) -> Option<std::borrow::Cow<'a, str>> {
	url.query_pairs()
		.find(|(name, _)| name == key)
		.map(|(_, value)| value)
}

fn youtube_id(candidate: &str) -> Option<&str> {
	(candidate.len() == YOUTUBE_ID_LEN
		&& candidate
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
	.then_some(candidate)
}

fn youtube(url: &Url, host: &str) -> Option<(String, Option<u64>)> {
	let path = segments(url);
	let id = if host == "youtu.be" {
		path.first().copied().and_then(youtube_id).map(str::to_owned)
	} else {
		match path.as_slice() {
			["watch"] => query(url, "v").and_then(|value| youtube_id(&value).map(str::to_owned)),
			["embed" | "shorts" | "live" | "v", id, ..] => youtube_id(id).map(str::to_owned),
			_ => None,
		}
	}?;
	let start = query(url, "t")
		.or_else(|| query(url, "start"))
		.and_then(|value| seconds(&value))
		.filter(|start| *start > 0 && *start <= MAX_START_SECONDS);
	Some((id, start))
}

fn seconds(value: &str) -> Option<u64> {
	if let Ok(plain) = value.trim_end_matches('s').parse::<u64>() {
		return Some(plain);
	}
	let mut total = 0u64;
	let mut number = 0u64;
	let mut seen = false;
	for character in value.chars() {
		match character {
			'0'..='9' => {
				number = number
					.checked_mul(10)?
					.checked_add(u64::from(character as u8 - b'0'))?;
				seen = true;
			}
			'h' | 'm' | 's' if seen => {
				let unit = match character {
					'h' => 3600,
					'm' => 60,
					_ => 1,
				};
				total = total.checked_add(number.checked_mul(unit)?)?;
				number = 0;
				seen = false;
			}
			_ => return None,
		}
	}
	(!seen).then_some(total)
}

fn vimeo(url: &Url) -> Option<String> {
	let path = segments(url);
	let id = match path.as_slice() {
		["video", id, ..] => *id,
		[id] => *id,
		["channels", _, id] => *id,
		_ => return None,
	};
	(!id.is_empty() && id.len() <= 12 && id.bytes().all(|byte| byte.is_ascii_digit())).then(|| id.to_owned())
}

fn spotify(url: &Url) -> Option<Player> {
	let path = segments(url);
	let path = match path.as_slice() {
		[locale, rest @ ..] if locale.starts_with("intl-") => rest,
		["embed", rest @ ..] => rest,
		rest => rest,
	};
	let [kind, id] = path else {
		return None;
	};
	let height = match *kind {
		"track" => SPOTIFY_TRACK_HEIGHT,
		"episode" => SPOTIFY_EPISODE_HEIGHT,
		"album" | "playlist" | "artist" | "show" => SPOTIFY_LIST_HEIGHT,
		_ => return None,
	};
	let valid = id.len() == SPOTIFY_ID_LEN && id.bytes().all(|byte| byte.is_ascii_alphanumeric());
	valid.then(|| Player {
		provider: Provider::Spotify,
		layout: Layout::Widget,
		src: format!("https://open.spotify.com/embed/{kind}/{id}"),
		width: SPOTIFY_WIDTH,
		height,
	})
}

fn soundcloud_segment(segment: &str) -> bool {
	!segment.is_empty()
		&& segment.len() <= MAX_SEGMENT_LEN
		&& segment
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn soundcloud(url: &Url) -> Option<Player> {
	let path = segments(url);
	let (canonical, height) = match path.as_slice() {
		[user, "sets", set] if soundcloud_segment(user) && soundcloud_segment(set) => (
			format!("https://soundcloud.com/{user}/sets/{set}"),
			SOUNDCLOUD_SET_HEIGHT,
		),
		[user, track] if soundcloud_segment(user) && soundcloud_segment(track) && *track != "sets" => (
			format!("https://soundcloud.com/{user}/{track}"),
			SOUNDCLOUD_TRACK_HEIGHT,
		),
		_ => return None,
	};
	let encoded: String = byte_serialize(canonical.as_bytes()).collect();
	Some(Player {
		provider: Provider::SoundCloud,
		layout: Layout::Widget,
		src: format!("https://w.soundcloud.com/player/?url={encoded}&auto_play=true&visual=false"),
		width: SOUNDCLOUD_WIDTH,
		height,
	})
}

pub fn frame_document(player: &Player) -> String {
	let frames = FRAME_ORIGINS.join(" ");
	let src = escape_attribute(&player.src);
	format!(
		"<!doctype html><html><head><meta charset=\"utf-8\">\
		<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; frame-src {frames}\">\
		<meta name=\"referrer\" content=\"strict-origin\">\
		<style>html,body{{margin:0;padding:0;width:100%;height:100%;overflow:hidden;background:#000}}iframe{{border:0;width:100%;height:100%;display:block}}</style>\
		</head><body><iframe src=\"{src}\" allow=\"autoplay; encrypted-media; picture-in-picture\" \
		sandbox=\"allow-scripts allow-same-origin allow-popups allow-popups-to-escape-sandbox allow-presentation\" \
		referrerpolicy=\"strict-origin\"></iframe></body></html>"
	)
}

fn escape_attribute(value: &str) -> String {
	let mut escaped = String::with_capacity(value.len());
	for character in value.chars() {
		match character {
			'&' => escaped.push_str("&amp;"),
			'"' => escaped.push_str("&quot;"),
			'<' => escaped.push_str("&lt;"),
			'>' => escaped.push_str("&gt;"),
			'\'' => escaped.push_str("&#39;"),
			other => escaped.push(other),
		}
	}
	escaped
}
