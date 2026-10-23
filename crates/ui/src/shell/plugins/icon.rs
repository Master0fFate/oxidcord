use std::collections::HashSet;
use std::sync::LazyLock;

use gpui::{App, Hsla, IntoElement, Rems, RenderOnce, SharedString, Styled as _, Window, svg};
use gpui_kit_assets::IconName;

use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const MAX_NAME_BYTES: usize = 48;
const DEFAULT_SIZE: f32 = 16.;

static KNOWN: LazyLock<HashSet<SharedString>> =
	LazyLock::new(|| IconName::ALL.iter().map(|icon| icon.path()).collect());

pub fn resolve(name: &str) -> Option<SharedString> {
	let valid = !name.is_empty()
		&& name.len() <= MAX_NAME_BYTES
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
	if !valid {
		return None;
	}
	let path = SharedString::from(format!("icons/{name}.svg"));
	KNOWN.contains(&path).then_some(path)
}

#[derive(IntoElement)]
pub struct PluginIcon {
	name: SharedString,
	size: Rems,
	color: Option<Hsla>,
}

impl PluginIcon {
	pub fn new(name: impl Into<SharedString>) -> Self {
		Self {
			name: name.into(),
			size: rem(DEFAULT_SIZE),
			color: None,
		}
	}

	pub fn size(mut self, size: Rems) -> Self {
		self.size = size;
		self
	}

	pub fn color(mut self, color: Hsla) -> Self {
		self.color = Some(color);
		self
	}
}

impl RenderOnce for PluginIcon {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let color = self.color.unwrap_or(cx.global::<Theme>().interactive_normal);
		match resolve(&self.name) {
			Some(path) => svg()
				.path(path)
				.flex_shrink_0()
				.size(self.size)
				.text_color(color)
				.into_any_element(),
			None => Icon::new(IconKind::Plugins)
				.size(self.size)
				.color(color)
				.into_any_element(),
		}
	}
}
