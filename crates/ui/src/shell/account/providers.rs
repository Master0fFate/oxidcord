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
	link: Link,
}

const PROVIDERS: &[Provider] = &[
	Provider {
		kind: "amazon-music",
		label: "Amazon Music",
		link: Link::None,
	},
	Provider {
		kind: "bluesky",
		label: "Bluesky",
		link: Link::Name("https://bsky.app/profile"),
	},
	Provider {
		kind: "bungie",
		label: "Bungie.net",
		link: Link::None,
	},
	Provider {
		kind: "contacts",
		label: "Contacts",
		link: Link::None,
	},
	Provider {
		kind: "crunchyroll",
		label: "Crunchyroll",
		link: Link::None,
	},
	Provider {
		kind: "domain",
		label: "Domain",
		link: Link::Host,
	},
	Provider {
		kind: "ebay",
		label: "eBay",
		link: Link::Name("https://www.ebay.com/usr"),
	},
	Provider {
		kind: "epicgames",
		label: "Epic Games",
		link: Link::None,
	},
	Provider {
		kind: "facebook",
		label: "Facebook",
		link: Link::Id("https://www.facebook.com"),
	},
	Provider {
		kind: "github",
		label: "GitHub",
		link: Link::Name("https://github.com"),
	},
	Provider {
		kind: "instagram",
		label: "Instagram",
		link: Link::Name("https://www.instagram.com"),
	},
	Provider {
		kind: "mastodon",
		label: "Mastodon",
		link: Link::None,
	},
	Provider {
		kind: "paypal",
		label: "PayPal",
		link: Link::None,
	},
	Provider {
		kind: "playstation",
		label: "PlayStation",
		link: Link::None,
	},
	Provider {
		kind: "reddit",
		label: "Reddit",
		link: Link::Name("https://www.reddit.com/user"),
	},
	Provider {
		kind: "roblox",
		label: "Roblox",
		link: Link::None,
	},
	Provider {
		kind: "samsung",
		label: "Samsung Galaxy",
		link: Link::None,
	},
	Provider {
		kind: "skype",
		label: "Skype",
		link: Link::None,
	},
	Provider {
		kind: "soundcloud",
		label: "SoundCloud",
		link: Link::Name("https://soundcloud.com"),
	},
	Provider {
		kind: "spotify",
		label: "Spotify",
		link: Link::Id("https://open.spotify.com/user"),
	},
	Provider {
		kind: "steam",
		label: "Steam",
		link: Link::Id("https://steamcommunity.com/profiles"),
	},
	Provider {
		kind: "tiktok",
		label: "TikTok",
		link: Link::Handle("https://www.tiktok.com"),
	},
	Provider {
		kind: "twitch",
		label: "Twitch",
		link: Link::Name("https://www.twitch.tv"),
	},
	Provider {
		kind: "twitter",
		label: "X",
		link: Link::Name("https://x.com"),
	},
	Provider {
		kind: "xbox",
		label: "Xbox",
		link: Link::None,
	},
	Provider {
		kind: "youtube",
		label: "YouTube",
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
