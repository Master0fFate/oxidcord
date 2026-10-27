use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use media::providers::{self, Candidate, Layout, Provider};
use media::{Decoded, Limits, MediaFetcher};
use mockcord::Fixtures;
use player::{Options, Player, State, Status};
use protocol::{EmbedKind, Message, Snowflake};
use serde_json::Value;
use url::Url;

use super::Harness;

const PLAY_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(10);
const SEEK_AFTER: Duration = Duration::from_millis(600);
const SEEK_TO: Duration = Duration::from_millis(300);
const ANIMATED_FLAG: u64 = 1 << 5;
const MAX_SIZE: (u32, u32) = (640, 360);
const YOUTUBE_SRC: &str = "https://www.youtube.com/embed/dQw4w9WgXcQ?autoplay=1&auto_play=1";
const SPOTIFY_SRC: &str = "https://open.spotify.com/embed/track/4cOdK2wGLETKBW3PvgPWqT";
const HOSTILE_LINKS: [&str; 10] = [
	"javascript:alert(1)",
	"http://www.youtube.com/watch?v=dQw4w9WgXcQ",
	"https://user:pass@www.youtube.com/watch?v=dQw4w9WgXcQ",
	"https://www.youtube.com.evil.example/watch?v=dQw4w9WgXcQ",
	"https://evil.example/embed/dQw4w9WgXcQ",
	"https://www.youtube.com/watch?v=dQw4w9WgXcQ\"><script>",
	"https://www.youtube.com/embed/..%2F..%2Fevil",
	"https://open.spotify.com/track/..%2F..%2Fevil",
	"https://open.spotify.com/embed/track/4cOdK2wGLETKBW3PvgPWqT/../../x",
	"https://soundcloud.com/user/track\"onload=alert(1)",
];

pub(super) struct MediaOutcome {
	pub(super) videos_played: usize,
	pub(super) audio_played: usize,
	pub(super) gif_frames: usize,
	pub(super) codec_pack: bool,
}

struct Played {
	frames: usize,
	status: Status,
	timed_while_playing: bool,
}

fn fixture_file(fixtures: &Fixtures, file: &str) -> Option<(Snowflake, Snowflake)> {
	fixtures.messages.iter().find_map(|(channel_id, messages)| {
		messages.iter().find_map(|(id, value)| {
			value
				.get("attachments")?
				.as_array()?
				.iter()
				.any(|attachment| attachment.get("filename").and_then(Value::as_str) == Some(file))
				.then_some((Snowflake(*channel_id), Snowflake(*id)))
		})
	})
}

fn fixture_link(fixtures: &Fixtures, prefix: &str) -> Option<(Snowflake, Snowflake)> {
	fixtures.messages.iter().find_map(|(channel_id, messages)| {
		messages.iter().find_map(|(id, value)| {
			value
				.get("content")?
				.as_str()?
				.starts_with(prefix)
				.then_some((Snowflake(*channel_id), Snowflake(*id)))
		})
	})
}

fn mock_media(origin: &Url, url: &str) -> Result<String> {
	let file = Url::parse(url)
		.ok()
		.and_then(|parsed| parsed.path_segments()?.next_back().map(str::to_owned))
		.context("media url has no file name")?;
	Ok(format!("{}media/e2e-{file}", origin))
}

fn play(fetcher: &MediaFetcher, url: String, seek: bool) -> Result<Played> {
	let runtime = tokio::runtime::Handle::current();
	let player = Player::open(
		fetcher.clone(),
		runtime,
		url.clone(),
		Options {
			autoplay: true,
			looping: false,
			volume: 0.,
			max_size: MAX_SIZE,
		},
		Arc::new(|| {}),
	);
	let started = Instant::now();
	let mut frames = 0;
	let mut seeked = !seek;
	let mut timed_while_playing = false;
	loop {
		if player.take_frame().is_some() {
			frames += 1;
		}
		let status = player.status();
		timed_while_playing |= status.state == State::Playing && status.duration.is_some();
		if !seeked && status.position >= SEEK_AFTER {
			player.seek(SEEK_TO);
			seeked = true;
		}
		match status.state {
			State::Ended => {
				return Ok(Played {
					frames,
					status,
					timed_while_playing,
				});
			}
			State::Failed => bail!("{url} failed to play"),
			_ if started.elapsed() > PLAY_TIMEOUT => bail!("{url} did not finish playing"),
			_ => std::thread::sleep(POLL),
		}
	}
}

fn check_providers(youtube: &Message, spotify: &Message) -> Result<()> {
	let resolve = |message: &Message| {
		let embed = message.embeds.first()?;
		let video = embed.video.as_ref();
		providers::player(&Candidate {
			url: embed.url.as_deref(),
			video_url: video.and_then(|video| video.url.as_deref()),
			video_size: None,
		})
	};
	let youtube = resolve(youtube).context("the youtube embed did not resolve to a player")?;
	ensure!(
		youtube.provider == Provider::YouTube
			&& youtube.layout == Layout::Video
			&& youtube.src == YOUTUBE_SRC,
		"the youtube player is wrong: {youtube:?}"
	);
	let spotify = resolve(spotify).context("the spotify embed did not resolve to a player")?;
	ensure!(
		spotify.provider == Provider::Spotify
			&& spotify.layout == Layout::Widget
			&& spotify.src == SPOTIFY_SRC,
		"the spotify player is wrong: {spotify:?}"
	);
	for link in HOSTILE_LINKS {
		let resolved = providers::player(&Candidate {
			url: Some(link),
			video_url: Some(link),
			video_size: None,
		});
		ensure!(resolved.is_none(), "a hostile link resolved to a player: {link}");
	}
	let document = providers::frame_document(&providers::Player {
		src: "https://www.youtube.com/embed/x\"></iframe><script>alert(1)</script>".to_owned(),
		..youtube
	});
	ensure!(
		!document.contains("<script") && document.contains("frame-src") && document.contains("sandbox="),
		"the embed frame document is not locked down"
	);
	Ok(())
}

impl Harness {
	fn cached_message(&self, (channel_id, message_id): (Snowflake, Snowflake)) -> Result<Message> {
		self.state
			.read()
			.messages(channel_id)
			.and_then(|cache| cache.by_id.get(&message_id))
			.map(|message| (**message).clone())
			.context("a media message is missing from the cache")
	}

	async fn load(&self, found: Option<(Snowflake, Snowflake)>, what: &str) -> Result<Message> {
		let (channel_id, message_id) = found.with_context(|| format!("the fixtures have no {what}"))?;
		self.reveal(channel_id, message_id).await?;
		self.cached_message((channel_id, message_id))
	}

	pub(super) async fn media_features(&self, fixtures: &Fixtures, rest_base: &str) -> Result<MediaOutcome> {
		let origin = Url::parse(rest_base)?.join("/")?;
		let fetcher = MediaFetcher::builder()
			.allow_loopback_http(origin.port().context("mock server has no port")?)
			.build()?;
		let video = self
			.load(fixture_file(fixtures, "clip.mp4"), "video file")
			.await?;
		let webm = self
			.load(fixture_file(fixtures, "clip.webm"), "webm file")
			.await?;
		let song = self
			.load(fixture_file(fixtures, "song.mp3"), "audio file")
			.await?;
		let voice = self
			.load(fixture_file(fixtures, "voice.ogg"), "voice message")
			.await?;
		let gif = self.load(fixture_file(fixtures, "loop.gif"), "gif file").await?;
		let gif_link = self
			.load(
				fixture_link(fixtures, "https://example.com/reactions/"),
				"gif link",
			)
			.await?;
		let gifv = self
			.load(fixture_link(fixtures, "https://tenor.com/view/"), "gifv")
			.await?;
		let youtube = self
			.load(
				fixture_link(fixtures, "https://www.youtube.com/watch"),
				"youtube link",
			)
			.await?;
		let spotify = self
			.load(
				fixture_link(fixtures, "https://open.spotify.com/"),
				"spotify link",
			)
			.await?;

		let attachment_url = |message: &Message| -> Result<String> {
			let attachment = message
				.attachments
				.first()
				.context("message lost its attachment")?;
			mock_media(
				&origin,
				attachment.url.as_deref().context("attachment has no url")?,
			)
		};
		ensure!(
			voice
				.attachments
				.first()
				.is_some_and(|attachment| attachment.waveform.is_some()),
			"the voice message lost its waveform"
		);
		ensure!(
			gif.attachments
				.first()
				.and_then(|attachment| attachment.flags)
				.is_some_and(|flags| flags & ANIMATED_FLAG != 0),
			"the gif attachment is not flagged as animated"
		);
		let link_media = gif_link
			.embeds
			.first()
			.filter(|embed| embed.kind == Some(EmbedKind::Image))
			.and_then(|embed| embed.thumbnail.clone())
			.context("the gif link has no image embed")?;
		ensure!(
			link_media.flags.is_some_and(|flags| flags & ANIMATED_FLAG != 0),
			"the gif link embed is not flagged as animated"
		);
		let proxied = media::cdn::animated_proxy(
			"https://cdn.discordapp.com/attachments/1/2/loop.gif",
			MAX_SIZE.0,
			MAX_SIZE.1,
		);
		ensure!(
			proxied.contains("animated=true") && proxied.starts_with("https://media.discordapp.net/"),
			"gifs are not requested animated from the media proxy"
		);
		let gif_bytes = fetcher.fetch(&attachment_url(&gif)?).await?;
		let gif_frames = match media::decode(&gif_bytes, &Limits::default())? {
			Decoded::Animated(animation) => animation.frames.len(),
			Decoded::Still(_) => bail!("the gif decoded to a single frame"),
		};
		let gifv_url = gifv
			.embeds
			.first()
			.and_then(|embed| embed.video.as_ref())
			.and_then(|video| video.url.clone())
			.context("the gifv embed has no video")?;
		check_providers(&youtube, &spotify)?;

		let codec_pack = player::codec_pack_available();
		let mut jobs = vec![
			(attachment_url(&video)?, true, true),
			(mock_media(&origin, &gifv_url)?, true, false),
			(attachment_url(&song)?, false, true),
			(attachment_url(&voice)?, false, false),
		];
		if codec_pack {
			jobs.push((attachment_url(&webm)?, true, false));
		}
		let played = tokio::task::spawn_blocking(move || {
			jobs.into_iter()
				.map(|(url, is_video, seek)| play(&fetcher, url, seek).map(|played| (is_video, played)))
				.collect::<Result<Vec<_>>>()
		})
		.await
		.context("the playback check panicked")??;
		let mut outcome = MediaOutcome {
			videos_played: 0,
			audio_played: 0,
			gif_frames,
			codec_pack,
		};
		for (is_video, played) in played {
			ensure!(
				played.timed_while_playing,
				"a file played without a known duration, so its seek bar cannot move"
			);
			if is_video {
				ensure!(
					played.frames > 1 && played.status.video.is_some(),
					"a video ended without showing frames"
				);
				outcome.videos_played += 1;
			} else {
				ensure!(
					played.status.audio && played.status.duration.is_some(),
					"an audio file ended without an audio track"
				);
				outcome.audio_played += 1;
			}
		}
		Ok(outcome)
	}
}
