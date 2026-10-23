use rand::Rng;
use rand::rngs::StdRng;

pub const ADJECTIVES: &[&str] = &[
	"swift", "quiet", "brave", "lucky", "cosmic", "rusty", "mellow", "fuzzy", "gentle", "chilly", "witty",
	"sunny", "clever", "electric", "velvet", "tiny",
];

pub const NOUNS: &[&str] = &[
	"fox", "otter", "falcon", "panda", "comet", "badger", "lynx", "heron", "gecko", "moth", "ember", "pixel",
	"cedar", "orbit", "koala", "wolf",
];

pub const GUILD_NAMES: &[&str] = &[
	"Rustaceans Hideout",
	"Pixel Lounge",
	"Midnight Coders",
	"Garden Club",
	"Synth Collective",
	"Retro Arcade",
	"Open Source Hub",
	"Late Night Radio",
];

pub const TEXT_CHANNEL_NAMES: &[&str] = &[
	"general",
	"random",
	"dev-chat",
	"memes",
	"music",
	"support",
	"off-topic",
	"showcase",
	"gaming",
	"food",
	"pets",
	"art",
];

pub const SENTENCES: &[&str] = &[
	"just pushed the fix, can someone double check the build on windows",
	"honestly the new layout feels way better than the old one",
	"does anyone know why the cache keeps getting invalidated after a restart",
	"i spent the whole weekend refactoring the parser and it was worth it",
	"the latest release finally fixes the flicker when scrolling fast",
	"we should probably split that module before it gets any bigger",
	"can we move the standup to thursday, i have a dentist thing",
	"that bug only shows up when the window is resized while loading",
	"found a really nice crate for this, zero allocations in the hot path",
	"i keep forgetting to run the formatter before committing",
	"who is up for a game later tonight, i just got off work",
	"the docs are missing a section about configuring the proxy",
	"benchmarks look great, about 40 percent faster on the big channel",
	"my keyboard died so i am typing this on my phone, sorry for typos",
	"the playlist for tonight is ready, mostly synthwave and some lo-fi",
	"remember to rotate your tokens, the old ones expire next week",
	"that screenshot is hilarious, who even made that theme",
	"i think the scheduler is starving the low priority tasks",
	"reproduced it on a clean install, attaching logs in the thread",
	"coffee first, then we can talk about the roadmap",
	"the new emoji pack is live, go crazy",
	"anyone else getting a weird gray bar on the left side of the window",
	"merged, thanks for the quick review everyone",
	"the trick is to batch the writes and flush once per frame",
	"we are at 3 open issues and one of them is just a typo",
	"i can take the migration, just need a day or two",
	"that sounds good to me, let us ship it on monday",
	"the long weekend really helped, back to full speed now",
];

pub const SHORT_REPLIES: &[&str] = &[
	"lol",
	"same",
	"fr",
	"nice",
	"agreed",
	"wait what",
	"oh no",
	"gg",
	"true",
	"makes sense",
	"on it",
	"thanks!",
	"yep",
	"nope",
	"hmm",
	"that works",
	"W",
	"ngl that is clean",
	"brb",
	"can confirm",
];

pub const UNICODE_EMOJI: &[&str] = &["😂", "🔥", "👍", "❤️", "🎉", "👀", "🦀", "✨", "🙏", "😅"];

pub const LINKS: &[&str] = &[
	"https://docs.rs/axum/latest/axum/",
	"https://github.com/rust-lang/rust/issues/1",
	"https://example.com/articles/lenient-decoding",
	"https://www.rust-lang.org/learn",
	"https://example.org/blog/2026/10/release-notes",
];

pub const GAMES: &[&str] = &[
	"Elden Ring",
	"Factorio",
	"Hades II",
	"Balatro",
	"Minecraft",
	"Celeste",
];

pub const SONGS: &[(&str, &str, &str)] = &[
	("Midnight City", "M83", "Hurry Up, We're Dreaming"),
	("Nightcall", "Kavinsky", "OutRun"),
	("Resonance", "HOME", "Odyssey"),
	("Genesis", "Grimes", "Visions"),
];

pub const CUSTOM_STATUSES: &[(&str, &str)] = &[
	("touching grass", "🌱"),
	("in the zone", "🎧"),
	("brb coffee", "☕"),
	("shipping friday", "🚀"),
];

pub fn pick<'a, T>(rng: &mut StdRng, items: &'a [T]) -> &'a T {
	&items[rng.random_range(0..items.len())]
}

pub fn hex_hash(rng: &mut StdRng) -> String {
	format!("{:032x}", rng.random::<u128>())
}
