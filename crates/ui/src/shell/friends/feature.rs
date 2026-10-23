use gpui::{Context, FontWeight, IntoElement, ParentElement as _, Render, Styled as _, Window, div};
use gpui_component::button::{Button, ButtonVariants as _};

use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureKind {
	Nitro,
	Shop,
}

impl FeatureKind {
	fn name_key(self) -> &'static str {
		match self {
			Self::Nitro => "dm-nitro",
			Self::Shop => "dm-shop",
		}
	}

	fn note_key(self) -> &'static str {
		match self {
			Self::Nitro => "feature-nitro-note",
			Self::Shop => "feature-shop-note",
		}
	}

	fn url(self) -> &'static str {
		match self {
			Self::Nitro => "https://discord.com/nitro",
			Self::Shop => "https://discord.com/shop",
		}
	}

	fn icon(self) -> IconKind {
		match self {
			Self::Nitro => IconKind::Nitro,
			Self::Shop => IconKind::Shop,
		}
	}
}

pub struct FeaturePage {
	kind: FeatureKind,
}

impl FeaturePage {
	pub fn new(kind: FeatureKind) -> Self {
		Self { kind }
	}

	pub fn show(&mut self, kind: FeatureKind, cx: &mut Context<Self>) {
		self.kind = kind;
		cx.notify();
	}
}

impl Render for FeaturePage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let name = cx.t(self.kind.name_key());
		let url = self.kind.url();
		div()
			.flex()
			.flex_col()
			.size_full()
			.items_center()
			.justify_center()
			.gap(theme.space_16)
			.bg(theme.background_primary)
			.child(
				Icon::new(self.kind.icon())
					.size(theme.avatar_lg)
					.color(theme.interactive_muted),
			)
			.child(
				div()
					.text_size(theme.font_xl)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(name.clone()),
			)
			.child(
				div()
					.text_size(theme.font_md)
					.text_color(theme.text_muted)
					.child(cx.t(self.kind.note_key())),
			)
			.child(
				Button::new("feature-open")
					.primary()
					.label(cx.t("feature-open"))
					.on_click(move |_, _, _| {
						if let Err(error) = platform::open::open_url(url) {
							tracing::warn!(%error, "could not open the page");
						}
					}),
			)
	}
}
