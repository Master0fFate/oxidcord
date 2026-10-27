use protocol::{Embed, EmbedKind, EmbedMedia, EmbedProvider};

struct Preview {
	url: &'static str,
	site: &'static str,
	title: &'static str,
	description: &'static str,
	color: Option<i64>,
	image: Option<(&'static str, i64, i64)>,
}

const PREVIEWS: &[Preview] = &[
	Preview {
		url: "https://www.rust-lang.org/learn",
		site: "Rust Programming Language",
		title: "Learn Rust",
		description: "A language empowering everyone to build reliable and efficient software.",
		color: Some(0x000000),
		image: Some((
			"https://www.rust-lang.org/static/images/rust-social-wide.jpg",
			1200,
			630,
		)),
	},
	Preview {
		url: "https://docs.rs/axum/latest/axum/",
		site: "docs.rs",
		title: "axum - Rust",
		description: "axum is a web application framework that focuses on ergonomics and modularity.",
		color: None,
		image: None,
	},
	Preview {
		url: "https://github.com/rust-lang/rust/issues/1",
		site: "GitHub",
		title: "Issue #1 · rust-lang/rust",
		description: "Empowering everyone to build reliable and efficient software.",
		color: Some(0x1f2328),
		image: Some((
			"https://opengraph.githubassets.com/mock/rust-lang/rust/issues/1",
			1200,
			600,
		)),
	},
	Preview {
		url: "https://example.org/blog/2026/10/release-notes",
		site: "Example Blog",
		title: "Release notes for October 2026",
		description: "Everything that shipped this month, from faster startup to a refreshed look.",
		color: Some(0x5865f2),
		image: None,
	},
];

fn proxied(url: &str) -> String {
	url.replace(
		"https://",
		"https://images-ext.discordapp.net/external/mock/https/",
	)
}

pub fn embeds_for(content: &str) -> Vec<Embed> {
	content
		.split_whitespace()
		.filter(|word| word.starts_with("https://") || word.starts_with("http://"))
		.map(|link| link.trim_end_matches(['.', ',', ')', '!', '?']))
		.map(embed_for)
		.collect()
}

fn embed_for(link: &str) -> Embed {
	match PREVIEWS.iter().find(|preview| preview.url == link) {
		Some(preview) => Embed {
			kind: Some(EmbedKind::Article),
			url: Some(link.to_owned()),
			title: Some(preview.title.to_owned()),
			description: Some(preview.description.to_owned()),
			color: preview.color,
			provider: Some(EmbedProvider {
				name: Some(preview.site.to_owned()),
				url: None,
			}),
			image: preview.image.map(|(url, width, height)| EmbedMedia {
				url: Some(url.to_owned()),
				proxy_url: Some(proxied(url)),
				width: Some(width),
				height: Some(height),
				..EmbedMedia::default()
			}),
			..Embed::default()
		},
		None => {
			let host = link
				.split("://")
				.nth(1)
				.and_then(|rest| rest.split('/').next())
				.unwrap_or(link);
			Embed {
				kind: Some(EmbedKind::Link),
				url: Some(link.to_owned()),
				title: Some(host.to_owned()),
				provider: Some(EmbedProvider {
					name: Some(host.to_owned()),
					url: None,
				}),
				..Embed::default()
			}
		}
	}
}
