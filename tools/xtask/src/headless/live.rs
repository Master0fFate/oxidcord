use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use media::cdn::{GifTile, gif_tile};
use media::{Decoded, Limits, MediaError, MediaFetcher, MediaService, SizeHint};
use protocol::Gif;
use serde_json::json;
use session::Command;
use store::{Paths, Settings, Store, TokenStore};
use tokio::time::Instant;

use super::Harness;

const GIF_QUERY: &str = "cat";
const SUGGEST_QUERY: &str = "ha";
const TILE_WIDTH: u32 = 160;
const MIN_TILE: u32 = 64;
const MAX_TILE: u32 = 240;
const ANIMATED_SAMPLES: usize = 8;
const UPSTREAM_STATUS: &str = ": status ";

struct MediaRun {
	loaded: usize,
	unavailable: usize,
	failed: Vec<String>,
	frames: usize,
	slowest: Duration,
}

impl MediaRun {
	fn report(&self) -> serde_json::Value {
		json!({
			"loaded": self.loaded,
			"unavailable": self.unavailable,
			"failed": self.failed.len(),
			"frames": self.frames,
			"slowest_ms": self.slowest.as_millis(),
		})
	}
}

fn saved_token(paths: &Paths, settings: &Settings) -> Result<String> {
	TokenStore::new(paths.token_path(), settings.storage.plaintext_token)
		.load()
		.context("could not read the saved token")?
		.map(|token| token.trim().to_owned())
		.filter(|token| !token.is_empty())
		.context("no saved token, sign in with the desktop app first")
}

fn tile_hint(gif: &Gif) -> SizeHint {
	let height = if gif.width == 0 {
		TILE_WIDTH
	} else {
		(u64::from(TILE_WIDTH) * u64::from(gif.height) / u64::from(gif.width)) as u32
	};
	SizeHint::new(TILE_WIDTH, height.clamp(MIN_TILE, MAX_TILE))
}

fn tile(gif: &Gif) -> (GifTile, SizeHint) {
	let hint = tile_hint(gif);
	let source = if gif.gif_src.is_empty() {
		&gif.src
	} else {
		&gif.gif_src
	};
	(gif_tile(source, &gif.preview, hint.width, hint.height), hint)
}

fn frame_count(decoded: &Decoded) -> usize {
	match decoded {
		Decoded::Still(_) => 1,
		Decoded::Animated(animation) => animation.frames.len(),
	}
}

async fn load_all(
	media: &MediaService,
	sources: impl Iterator<Item = (String, SizeHint)>,
	still: bool,
) -> MediaRun {
	let mut run = MediaRun {
		loaded: 0,
		unavailable: 0,
		failed: Vec::new(),
		frames: 0,
		slowest: Duration::ZERO,
	};
	for (url, hint) in sources {
		let started = Instant::now();
		match media.load(&url, hint, still).await {
			Ok(decoded) => {
				run.loaded += 1;
				run.frames += frame_count(&decoded);
			}
			Err(MediaError::Fetch(reason)) if reason.contains(UPSTREAM_STATUS) => run.unavailable += 1,
			Err(error) => run.failed.push(format!("{}: {error}", media::redact_url(&url))),
		}
		run.slowest = run.slowest.max(started.elapsed());
	}
	run
}

fn media_service(settings: &Settings, root: &Path) -> Result<MediaService> {
	let fetcher = MediaFetcher::builder()
		.user_agent(protocol::gateway::chrome_user_agent(
			&settings.client.chrome_version,
		))
		.build()
		.context("could not build the media fetcher")?;
	Ok(MediaService::new(fetcher, root.join("media"), Limits::default()))
}

fn favorite_urls(harness: &Harness) -> Vec<String> {
	harness
		.state
		.read()
		.gif_favorites()
		.map(|gifs| gifs.iter().map(|gif| gif.url.clone()).collect())
		.unwrap_or_default()
}

async fn start(store: &Arc<Store>, token: &str) -> Result<Harness> {
	let harness = Harness::start(
		store.clone(),
		rest::RestConfig::default().base_url.as_str(),
		gateway::GatewayConfig::DEFAULT_GATEWAY_URL,
		token,
	)?;
	harness
		.until("the gateway to become ready", |state| {
			state.current_user().is_some() && state.session().connected
		})
		.await?;
	harness.session.send(Command::LoadGifFavorites);
	harness
		.until("gif favorites to load", |state| state.gif_favorites().is_some())
		.await?;
	Ok(harness)
}

pub(super) async fn run(root: &Path) -> Result<()> {
	let real = Paths::discover().context("could not find the Oxidcord data folder")?;
	let settings = Settings::load(&real);
	let token = saved_token(&real, &settings)?;
	let store = Arc::new(Store::open(Paths::at(root)?)?);
	store.update_settings(|scratch| {
		scratch.storage.plaintext_token = true;
		scratch.locale.clone_from(&settings.locale);
		scratch.client = settings.client.clone();
	})?;

	let harness = start(&store, &token).await?;
	let favorites_before = favorite_urls(&harness);
	harness.session.send(Command::LoadGifTrending);
	harness
		.until("trending gifs and categories to load", |state| {
			state
				.gif_trending()
				.is_some_and(|trending| !trending.gifs.is_empty() && !trending.categories.is_empty())
		})
		.await?;
	harness.session.send(Command::SearchGifs {
		query: GIF_QUERY.to_owned(),
	});
	harness.session.send(Command::SuggestGifs {
		query: SUGGEST_QUERY.to_owned(),
	});
	harness
		.until("gif search results and suggestions", |state| {
			state.gif_search(GIF_QUERY).is_some_and(|gifs| !gifs.is_empty())
				&& state.gif_suggestions(SUGGEST_QUERY).is_some()
		})
		.await?;

	let (trending, categories, results) = {
		let state = harness.state.read();
		let trending = state.gif_trending().context("trending gifs vanished")?;
		(
			trending.gifs.to_vec(),
			trending.categories.to_vec(),
			state
				.gif_search(GIF_QUERY)
				.map(|gifs| gifs.to_vec())
				.unwrap_or_default(),
		)
	};
	let favorite = trending
		.iter()
		.find(|gif| !favorites_before.contains(&gif.url))
		.cloned()
		.context("every trending gif is already a favorite")?;
	let favorite_url = favorite.url.clone();
	harness.session.send(Command::FavoriteGif { gif: favorite });
	harness
		.until("the gif to be favorited", |state| {
			state
				.gif_favorites()
				.is_some_and(|gifs| gifs.iter().any(|gif| gif.url == favorite_url))
		})
		.await?;
	let favorites = harness
		.state
		.read()
		.gif_favorites()
		.map(|gifs| gifs.to_vec())
		.unwrap_or_default();
	harness.session.send(Command::UnfavoriteGif {
		url: favorite_url.clone(),
	});
	harness
		.until("the gif to be unfavorited", |state| {
			state
				.gif_favorites()
				.is_some_and(|gifs| !gifs.iter().any(|gif| gif.url == favorite_url))
		})
		.await?;
	harness.stop().await?;

	let reloaded = start(&store, &token).await?;
	let favorites_after = favorite_urls(&reloaded);
	reloaded.stop().await?;
	ensure!(
		favorites_after == favorites_before,
		"favorites on the server changed after a favorite round trip ({} before, {} after)",
		favorites_before.len(),
		favorites_after.len()
	);

	let media = media_service(&settings, root)?;
	let stills = load_all(
		&media,
		trending.iter().chain(&results).chain(&favorites).map(|gif| {
			let (tile, hint) = tile(gif);
			(tile.still, hint)
		}),
		true,
	)
	.await;
	let category_stills = load_all(
		&media,
		categories.iter().map(|category| {
			let hint = SizeHint::new(TILE_WIDTH, MAX_TILE);
			(gif_tile(&category.src, "", hint.width, hint.height).still, hint)
		}),
		true,
	)
	.await;
	let animated = load_all(
		&media,
		trending
			.iter()
			.take(ANIMATED_SAMPLES)
			.chain(favorites.iter().take(ANIMATED_SAMPLES))
			.filter_map(|gif| {
				let (tile, hint) = tile(gif);
				tile.animated.map(|animated| (animated, hint))
			}),
		false,
	)
	.await;

	println!(
		"{}",
		json!({
			"trending": trending.len(),
			"categories": categories.len(),
			"search_results": results.len(),
			"favorites": favorites_before.len(),
			"favorite_round_trip": true,
			"stills": stills.report(),
			"category_stills": category_stills.report(),
			"animated": animated.report(),
		})
	);
	let failures: Vec<&String> = stills
		.failed
		.iter()
		.chain(&category_stills.failed)
		.chain(&animated.failed)
		.collect();
	ensure!(failures.is_empty(), "media failed to load: {failures:#?}");
	Ok(())
}
