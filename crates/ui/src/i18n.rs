use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use fluent_langneg::LanguageIdentifier as NegotiationId;
use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use gpui::{App, Global, SharedString};
use unic_langid::LanguageIdentifier;

const DEFAULT_LOCALE: &str = "en-US";

const LOCALES: &[(&str, &[&str])] = &[
	(
		"en-US",
		&[
			include_str!("../locales/en-US/main.ftl"),
			include_str!("../locales/en-US/community.ftl"),
		],
	),
	(
		"cs",
		&[
			include_str!("../locales/cs/main.ftl"),
			include_str!("../locales/cs/community.ftl"),
		],
	),
	(
		"de",
		&[
			include_str!("../locales/de/main.ftl"),
			include_str!("../locales/de/community.ftl"),
		],
	),
	(
		"es",
		&[
			include_str!("../locales/es/main.ftl"),
			include_str!("../locales/es/community.ftl"),
		],
	),
	(
		"fr",
		&[
			include_str!("../locales/fr/main.ftl"),
			include_str!("../locales/fr/community.ftl"),
		],
	),
	(
		"ja",
		&[
			include_str!("../locales/ja/main.ftl"),
			include_str!("../locales/ja/community.ftl"),
		],
	),
	(
		"pt-BR",
		&[
			include_str!("../locales/pt-BR/main.ftl"),
			include_str!("../locales/pt-BR/community.ftl"),
		],
	),
	(
		"ru",
		&[
			include_str!("../locales/ru/main.ftl"),
			include_str!("../locales/ru/community.ftl"),
		],
	),
	(
		"zh-CN",
		&[
			include_str!("../locales/zh-CN/main.ftl"),
			include_str!("../locales/zh-CN/community.ftl"),
		],
	),
];

type Bundle = FluentBundle<FluentResource>;

pub enum ArgValue {
	Text(String),
	Number(f64),
}

impl From<&str> for ArgValue {
	fn from(value: &str) -> Self {
		Self::Text(value.to_owned())
	}
}

impl From<String> for ArgValue {
	fn from(value: String) -> Self {
		Self::Text(value)
	}
}

impl From<usize> for ArgValue {
	fn from(value: usize) -> Self {
		Self::Number(value as f64)
	}
}

impl From<i64> for ArgValue {
	fn from(value: i64) -> Self {
		Self::Number(value as f64)
	}
}

pub struct Localizer {
	active: Bundle,
	fallback: Bundle,
}

impl Global for Localizer {}

impl Localizer {
	pub fn new(requested: &str) -> Self {
		let fallback = bundle_for(DEFAULT_LOCALE);
		let active = bundle_for(&negotiate(requested));
		Self { active, fallback }
	}

	pub fn install(self, cx: &mut App) {
		cx.set_global(self);
	}

	pub fn available() -> impl Iterator<Item = &'static str> {
		LOCALES.iter().map(|(locale, _)| *locale)
	}

	pub fn t(&self, key: &str) -> SharedString {
		self.format(key, None)
	}

	pub fn t_args(&self, key: &str, args: &[(&str, ArgValue)]) -> SharedString {
		let mut fluent_args = FluentArgs::new();
		for (name, value) in args {
			match value {
				ArgValue::Text(text) => fluent_args.set(*name, FluentValue::from(text.as_str())),
				ArgValue::Number(number) => fluent_args.set(*name, FluentValue::from(*number)),
			}
		}
		self.format(key, Some(&fluent_args))
	}

	fn format(&self, key: &str, args: Option<&FluentArgs>) -> SharedString {
		[&self.active, &self.fallback]
			.into_iter()
			.find_map(|bundle| {
				let pattern = bundle.get_message(key)?.value()?;
				let mut errors = Vec::new();
				Some(bundle.format_pattern(pattern, args, &mut errors).into_owned())
			})
			.unwrap_or_else(|| {
				tracing::warn!(key, "missing localization key");
				key.to_owned()
			})
			.into()
	}
}

pub trait Translate {
	fn t(&self, key: &str) -> SharedString;
	fn t_args(&self, key: &str, args: &[(&str, ArgValue)]) -> SharedString;
}

impl Translate for App {
	fn t(&self, key: &str) -> SharedString {
		self.global::<Localizer>().t(key)
	}

	fn t_args(&self, key: &str, args: &[(&str, ArgValue)]) -> SharedString {
		self.global::<Localizer>().t_args(key, args)
	}
}

fn negotiate(requested: &str) -> String {
	let Ok(requested) = requested.parse::<NegotiationId>() else {
		return DEFAULT_LOCALE.to_owned();
	};
	let available: Vec<NegotiationId> = LOCALES
		.iter()
		.filter_map(|(locale, _)| locale.parse().ok())
		.collect();
	let default: NegotiationId = DEFAULT_LOCALE.parse().expect("default locale parses");
	negotiate_languages(
		&[requested],
		&available,
		Some(&default),
		NegotiationStrategy::Lookup,
	)
	.first()
	.map_or_else(|| DEFAULT_LOCALE.to_owned(), |locale| locale.to_string())
}

fn bundle_for(locale: &str) -> Bundle {
	let sources = LOCALES
		.iter()
		.find(|(name, _)| *name == locale)
		.map_or(LOCALES[0].1, |(_, sources)| *sources);
	let language: LanguageIdentifier = locale.parse().expect("builtin locale parses");
	let mut bundle = FluentBundle::new_concurrent(vec![language]);
	bundle.set_use_isolating(false);
	for source in sources {
		let resource = FluentResource::try_new((*source).to_owned()).unwrap_or_else(|(resource, errors)| {
			tracing::warn!(
				locale,
				errors = errors.len(),
				"localization file has syntax errors"
			);
			resource
		});
		if let Err(errors) = bundle.add_resource(resource) {
			tracing::warn!(
				locale,
				errors = errors.len(),
				"localization file has duplicate keys"
			);
		}
	}
	bundle
}
