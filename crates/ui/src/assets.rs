use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

use gpui::{AssetSource, Result, SharedString};
use gpui_kit_assets::AllAssets;

const ICON_PREFIX: &str = "oxidcord/icons/";
const MAX_OVERRIDE_BYTES: u64 = 256 * 1024;

macro_rules! icon {
	($name:literal) => {
		(
			concat!("oxidcord/icons/", $name, ".svg"),
			include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice(),
		)
	};
}

macro_rules! brand {
	($name:literal) => {
		(
			concat!("oxidcord/brands/", $name, ".svg"),
			include_bytes!(concat!("../assets/brands/", $name, ".svg")).as_slice(),
		)
	};
}

const BRANDS: &[(&str, &[u8])] = &[
	brand!("bluesky"),
	brand!("bungie"),
	brand!("crunchyroll"),
	brand!("ebay"),
	brand!("epicgames"),
	brand!("facebook"),
	brand!("github"),
	brand!("instagram"),
	brand!("mastodon"),
	brand!("paypal"),
	brand!("playstation"),
	brand!("reddit"),
	brand!("roblox"),
	brand!("samsung"),
	brand!("soundcloud"),
	brand!("spotify"),
	brand!("steam"),
	brand!("tiktok"),
	brand!("twitch"),
	brand!("x"),
	brand!("youtube"),
];

const ICONS: &[(&str, &[u8])] = &[
	icon!("activity"),
	icon!("apps"),
	icon!("bell"),
	icon!("bell-off"),
	icon!("circle-plus"),
	icon!("code"),
	icon!("compass"),
	icon!("edit"),
	icon!("emoji"),
	icon!("flag"),
	icon!("folder"),
	icon!("food"),
	icon!("forum"),
	icon!("friends"),
	icon!("gif"),
	icon!("gift"),
	icon!("hash"),
	icon!("headphones"),
	icon!("headphones-off"),
	icon!("help"),
	icon!("id"),
	icon!("inbox"),
	icon!("lock"),
	icon!("megaphone"),
	icon!("members"),
	icon!("mic"),
	icon!("mic-off"),
	icon!("more"),
	icon!("nature"),
	icon!("objects"),
	icon!("oxidcord"),
	icon!("phone"),
	icon!("pin"),
	icon!("reply"),
	icon!("send"),
	icon!("settings"),
	icon!("stage"),
	icon!("sticker"),
	icon!("symbols"),
	icon!("thread"),
	icon!("trash"),
	icon!("travel"),
	icon!("video"),
	icon!("volume"),
	icon!("volume-off"),
];

pub struct Assets {
	overrides: PathBuf,
}

impl Assets {
	pub fn new(overrides: PathBuf) -> Self {
		Self { overrides }
	}

	fn user_icon(&self, path: &str) -> Option<Vec<u8>> {
		let file = self.overrides.join(path.strip_prefix(ICON_PREFIX)?);
		let size = fs::metadata(&file).ok()?.len();
		if size > MAX_OVERRIDE_BYTES {
			tracing::warn!(icon = path, "ignoring an icon override larger than 256 KB");
			return None;
		}
		fs::read(file).ok()
	}
}

impl AssetSource for Assets {
	fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
		let Some((_, bundled)) = ICONS.iter().chain(BRANDS).find(|(name, _)| *name == path) else {
			return AllAssets.load(path);
		};
		Ok(Some(match self.user_icon(path) {
			Some(user) => Cow::Owned(user),
			None => Cow::Borrowed(bundled),
		}))
	}

	fn list(&self, path: &str) -> Result<Vec<SharedString>> {
		let mut names = AllAssets.list(path)?;
		names.extend(
			ICONS
				.iter()
				.chain(BRANDS)
				.filter(|(name, _)| name.starts_with(path))
				.map(|(name, _)| SharedString::new_static(name)),
		);
		Ok(names)
	}
}
