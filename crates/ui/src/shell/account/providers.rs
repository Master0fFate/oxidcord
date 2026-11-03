use url::Url;

#[derive(Clone, Copy)]
enum Link {
	None,
	Name(&'static str),
	Handle(&'static str),
	Id(&'static str),
	Host,
}

struct Provider {
	kind: &'static str,
	label: &'static str,
	icon: Option<&'static str>,
	link: Link,
}

const PROVIDERS: &[Provider] = &[
	Provider {
		kind: "amazon-music",
		label: "Amazon Music",
		icon: None,
		link: Link::None,
	},
	Provider {
		kind: "bluesky",
		label: "Bluesky",
		icon: Some("oxidcord/brands/bluesky.svg"),
		link: Link::Name("https://bsky.app/profile"),
	},
	Provider {
		kind: "bungie",
		label: "Bungie.net",
		icon: Some("oxidcord/brands/bungie.svg"),
		link: Link::None,
	},
	Provider {
		kind: "contacts",
		label: "Contacts",
		icon: None,
		link: Link::None,
	},
	Provider {
		kind: "crunchyroll",
		label: "Crunchyroll",
		icon: Some("oxidcord/brands/crunchyroll.svg"),
		link: Link::None,
	},
	Provider {
		kind: "domain",
		label: "Domain",
		icon: None,
		link: Link::Host,
	},
	Provider {
		kind: "ebay",
		label: "eBay",
		icon: Some("oxidcord/brands/ebay.svg"),
		link: Link::Name("https://www.ebay.com/usr"),
	},
	Provider {
		kind: "epicgames",
		label: "Epic Games",
		icon: Some("oxidcord/brands/epicgames.svg"),
		link: Link::None,
	},
	Provider {
		kind: "facebook",
		label: "Facebook",
		icon: Some("oxidcord/brands/facebook.svg"),
		link: Link::Id("https://www.facebook.com"),
	},
	Provider {
		kind: "github",
		label: "GitHub",
		icon: Some("oxidcord/brands/github.svg"),
		link: Link::Name("https://github.com"),
	},
	Provider {
		kind: "instagram",
		label: "Instagram",
		icon: Some("oxidcord/brands/instagram.svg"),
		link: Link::Name("https://www.instagram.com"),
	},
	Provider {
		kind: "mastodon",
		label: "Mastodon",
		icon: Some("oxidcord/brands/mastodon.svg"),
		link: Link::None,
	},
	Provider {
		kind: "paypal",
		label: "PayPal",
		icon: Some("oxidcord/brands/paypal.svg"),
		link: Link::None,
	},
	Provider {
		kind: "playstation",
		label: "PlayStation",
		icon: Some("oxidcord/brands/playstation.svg"),
		link: Link::None,
	},
	Provider {
		kind: "reddit",
		label: "Reddit",
		icon: Some("oxidcord/brands/reddit.svg"),
		link: Link::Name("https://www.reddit.com/user"),
	},
	Provider {
		kind: "roblox",
		label: "Roblox",
		icon: Some("oxidcord/brands/roblox.svg"),
		link: Link::None,
	},
	Provider {
		kind: "samsung",
		label: "Samsung Galaxy",
		icon: Some("oxidcord/brands/samsung.svg"),
		link: Link::None,
	},
	Provider {
		kind: "skype",
		label: "Skype",
		icon: None,
		link: Link::None,
	},
	Provider {
		kind: "soundcloud",
		label: "SoundCloud",
		icon: Some("oxidcord/brands/soundcloud.svg"),
		link: Link::Name("https://soundcloud.com"),
	},
	Provider {
		kind: "spotify",
		label: "Spotify",
		icon: Some("oxidcord/brands/spotify.svg"),
		link: Link::Id("https://open.spotify.com/user"),
	},
	Provider {
		kind: "steam",
		label: "Steam",
		icon: Some("oxidcord/brands/steam.svg"),
		link: Link::Id("https://steamcommunity.com/profiles"),
	},
	Provider {
		kind: "tiktok",
		label: "TikTok",
		icon: Some("oxidcord/brands/tiktok.svg"),
		link: Link::Handle("https://www.tiktok.com"),
	},
	Provider {
		kind: "twitch",
		label: "Twitch",
		icon: Some("oxidcord/brands/twitch.svg"),
		link: Link::Name("https://www.twitch.tv"),
	},
	Provider {
		kind: "twitter",
		label: "X",
		icon: Some("oxidcord/brands/x.svg"),
		link: Link::Name("https://x.com"),
	},
	Provider {
		kind: "xbox",
		label: "Xbox",
		icon: None,
		link: Link::None,
	},
	Provider {
		kind: "youtube",
		label: "YouTube",
		icon: Some("oxidcord/brands/youtube.svg"),
		link: Link::Id("https://www.youtube.com/channel"),
	},
];

const SHOWS_ACTIVITY: [&str; 4] = ["spotify", "steam", "xbox", "playstation"];

fn provider(kind: &str) -> Option<&'static Provider> {
	PROVIDERS.iter().find(|provider| provider.kind == kind)
}

pub fn label(kind: &str) -> &str {
	provider(kind).map_or(kind, |provider| provider.label)
}

pub fn icon(kind: &str) -> Option<&'static str> {
	provider(kind)?.icon
}

pub fn initial(kind: &str) -> &str {
	label(kind).get(..1).unwrap_or_default()
}

pub fn shows_activity(kind: &str) -> bool {
	SHOWS_ACTIVITY.contains(&kind)
}

pub fn profile_url(kind: &str, id: &str, name: &str) -> Option<String> {
	let link = provider(kind)?.link;
	let (base, segment) = match link {
		Link::None => return None,
		Link::Name(base) => (base, name.to_owned()),
		Link::Handle(base) => (base, format!("@{name}")),
		Link::Id(base) => (base, id.to_owned()),
		Link::Host => return domain_url(name),
	};
	if segment.is_empty() {
		return None;
	}
	let mut url = Url::parse(base).ok()?;
	url.path_segments_mut().ok()?.push(&segment);
	Some(url.into())
}

fn domain_url(name: &str) -> Option<String> {
	let url = Url::parse(&format!("https://{name}")).ok()?;
	(url.host_str() == Some(name)).then(|| url.into())
}
