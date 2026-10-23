use std::fs;
use std::io;
use std::path::Path;

use gpui::{App, Global, Hsla, Rems, Rgba, SharedString, hsla, px, rems};
use gpui_component::{Theme as ComponentTheme, ThemeColor, ThemeMode};
use platform::system_theme::{self, SystemTheme};
use serde_json::{Map, Value};

pub const SYSTEM_THEME: &str = "system";
const BASE_REM: f32 = 16.;

pub fn rem(pixels: f32) -> Rems {
	rems(pixels / BASE_REM)
}

pub fn rem_pixels(scale: f32) -> gpui::Pixels {
	px(BASE_REM * scale)
}

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
	#[error("could not read theme file: {0}")]
	Io(#[from] io::Error),
	#[error("theme file is not valid json: {0}")]
	Json(#[from] serde_json::Error),
	#[error("theme file must be a json object")]
	NotAnObject,
	#[error("unknown base theme {0:?}")]
	UnknownBase(String),
}

macro_rules! tokens {
	(
		colors { $($color:ident = $default:literal,)+ }
		sizes { $($size:ident = $value:literal,)+ }
	) => {
		#[derive(Debug, Clone, PartialEq)]
		pub struct Theme {
			pub name: SharedString,
			pub ui_fonts: Vec<SharedString>,
			pub code_fonts: Vec<SharedString>,
			pub ui_font: SharedString,
			pub code_font: SharedString,
			$(pub $color: Hsla,)+
			$(pub $size: Rems,)+
			pub ui_scale: f32,
		}

		impl Theme {
			pub fn dark() -> Self {
				let ui_fonts = to_families(UI_FONTS);
				let code_fonts = to_families(CODE_FONTS);
				Self {
					name: "dark".into(),
					ui_font: ui_fonts[0].clone(),
					code_font: code_fonts[0].clone(),
					ui_fonts,
					code_fonts,
					$($color: parse_color($default).expect("builtin color token"),)+
					$($size: rem($value),)+
					ui_scale: 1.,
				}
			}

			fn set_color(&mut self, key: &str, value: Hsla) -> bool {
				match key {
					$(stringify!($color) => self.$color = value,)+
					_ => return false,
				}
				true
			}

			fn set_size(&mut self, key: &str, value: f32) -> bool {
				match key {
					$(stringify!($size) => self.$size = rem(value),)+
					_ => return false,
				}
				true
			}
		}
	};
}

const UI_FONTS: &[&str] = &[
	"gg sans",
	"Noto Sans",
	"Segoe UI",
	"Helvetica Neue",
	"Arial",
	"sans-serif",
];
const CODE_FONTS: &[&str] = &["Consolas", "Menlo", "monospace"];

tokens! {
	colors {
		background_primary = "#313338",
		background_secondary = "#2b2d31",
		background_tertiary = "#1e1f22",
		background_floating = "#111214",
		user_panel_background = "#232428",
		modifier_hover = "rgba(79, 84, 92, 0.16)",
		modifier_selected = "rgba(79, 84, 92, 0.24)",
		modifier_active = "rgba(79, 84, 92, 0.48)",
		text_normal = "#dbdee1",
		text_muted = "#949ba4",
		header_primary = "#f2f3f5",
		header_secondary = "#b5bac1",
		interactive_normal = "#b5bac1",
		interactive_hover = "#dbdee1",
		interactive_active = "#ffffff",
		interactive_muted = "#4e5058",
		brand = "#5865f2",
		brand_hover = "#4752c4",
		text_on_brand = "#ffffff",
		status_online = "#23a559",
		status_idle = "#f0b232",
		status_dnd = "#f23f43",
		status_offline = "#80848e",
		status_streaming = "#593695",
		mention_background = "rgba(88, 101, 242, 0.3)",
		mention_text = "#c9cdfb",
		link = "#00a8fc",
		danger = "#f23f43",
		danger_hover = "#da373c",
		success = "#23a559",
		warning = "#f0b232",
		spoiler = "#202225",
		code_background = "#2b2d31",
		code_border = "#1e1f22",
		scrollbar_thumb = "#1a1b1e",
		scrollbar_track = "#2b2d31",
		divider = "#3f4147",
		input_background = "#383a40",
		button_secondary = "#4e5058",
		button_secondary_hover = "#6d6f78",
		unread_pill = "#f2f3f5",
		mention_badge = "#f23f43",
		overlay = "rgba(0, 0, 0, 0.7)",
		speaking_ring = "#23a559",
		live_badge = "#f23f43",
	}
	sizes {
		title_bar_height = 22.,
		server_rail_width = 72.,
		server_icon = 48.,
		channel_sidebar_width = 240.,
		member_list_width = 240.,
		top_bar_height = 48.,
		user_panel_height = 52.,
		guild_header_height = 48.,
		channel_row_height = 32.,
		member_row_height = 42.,
		message_avatar = 40.,
		message_gutter = 16.,
		compact_row = 22.,
		font_tiny = 12.,
		font_xs = 13.,
		font_sm = 14.,
		font_md = 16.,
		font_lg = 20.,
		font_xl = 24.,
		radius_sm = 4.,
		radius_md = 8.,
		radius_lg = 16.,
		radius_xl = 24.,
		space_2 = 2.,
		space_4 = 4.,
		space_8 = 8.,
		space_12 = 12.,
		space_16 = 16.,
		space_20 = 20.,
		space_24 = 24.,
		space_32 = 32.,
		login_card_width = 784.,
		login_qr_column = 240.,
		login_qr_size = 184.,
		login_logo = 56.,
		login_logo_icon = 30.,
		icon_sm = 16.,
		icon_md = 20.,
		icon_lg = 24.,
		avatar_xs = 24.,
		avatar_sm = 32.,
		avatar_md = 40.,
		avatar_lg = 80.,
		avatar_xl = 120.,
		friends_row_height = 62.,
		friends_active_width = 360.,
		profile_popover_width = 300.,
		profile_modal_width = 600.,
		profile_banner_height = 60.,
		profile_banner_height_lg = 120.,
		modal_width = 440.,
		status_popover_width = 260.,
		badge_icon = 22.,
		chip_height = 24.,
		field_height = 36.,
		settings_card_width = 400.,
		list_max_height = 280.,
		voice_icon_sm = 16.,
		voice_icon_md = 20.,
		voice_icon_lg = 24.,
		voice_row_height = 28.,
		voice_row_avatar = 24.,
		voice_badge_icon = 14.,
		voice_button = 32.,
		voice_tile_width = 260.,
		voice_tile_avatar = 80.,
		voice_control = 48.,
		voice_strip_tile = 168.,
		voice_chat_width = 380.,
		voice_picker_width = 760.,
		voice_thumb_height = 120.,
		voice_toast_width = 340.,
		voice_menu_width = 240.,
		voice_field_width = 320.,
		voice_meter_height = 8.,
		glyph_sm = 16.,
		glyph_md = 20.,
		glyph_lg = 24.,
		portrait_sm = 24.,
		portrait_md = 32.,
		portrait_lg = 48.,
		control_height = 32.,
		list_row_height = 44.,
		thread_panel_width = 420.,
		popover_width = 400.,
		popover_height = 440.,
		picker_width = 456.,
		picker_height = 440.,
		picker_cell = 44.,
		picker_sticker = 104.,
		picker_rail = 48.,
		gif_gap = 4.,
		forum_card_width = 288.,
		forum_thumb_height = 160.,
		forum_thumb_width = 96.,
		settings_nav_width = 232.,
		settings_content_width = 760.,
		settings_swatch = 28.,
		modal_width_sm = 440.,
		modal_width_md = 540.,
		modal_width_lg = 700.,
		invite_card_width = 432.,
		invite_card_icon = 56.,
		role_dot = 12.,
		permission_row_height = 48.,
		manage_grid_cell = 108.,
		toast_width = 360.,
	}
}

const LIGHT_OVERRIDES: &[(&str, &str)] = &[
	("background_primary", "#ffffff"),
	("background_secondary", "#f2f3f5"),
	("background_tertiary", "#e3e5e8"),
	("background_floating", "#ffffff"),
	("user_panel_background", "#ebedef"),
	("modifier_hover", "rgba(116, 127, 141, 0.08)"),
	("modifier_selected", "rgba(116, 127, 141, 0.16)"),
	("modifier_active", "rgba(116, 127, 141, 0.24)"),
	("text_normal", "#313338"),
	("text_muted", "#5c5e66"),
	("header_primary", "#060607"),
	("header_secondary", "#4e5058"),
	("interactive_normal", "#4e5058"),
	("interactive_hover", "#313338"),
	("interactive_active", "#060607"),
	("interactive_muted", "#a4a6ab"),
	("mention_background", "rgba(88, 101, 242, 0.15)"),
	("mention_text", "#3c45a5"),
	("link", "#006ce7"),
	("spoiler", "#d4d6d9"),
	("code_background", "#f2f3f5"),
	("code_border", "#e3e5e8"),
	("scrollbar_thumb", "#c1c3c7"),
	("scrollbar_track", "#f2f3f5"),
	("divider", "#e1e2e4"),
	("input_background", "#ebedef"),
	("button_secondary", "#e3e5e8"),
	("button_secondary_hover", "#d7d9dc"),
	("unread_pill", "#060607"),
	("overlay", "rgba(0, 0, 0, 0.5)"),
];

const AMOLED_OVERRIDES: &[(&str, &str)] = &[
	("background_primary", "#000000"),
	("background_secondary", "#0a0a0b"),
	("background_tertiary", "#000000"),
	("background_floating", "#0a0a0b"),
	("user_panel_background", "#050506"),
	("code_background", "#0a0a0b"),
	("code_border", "#1e1f22"),
	("scrollbar_track", "#000000"),
	("divider", "#1e1f22"),
	("input_background", "#141416"),
	("overlay", "rgba(0, 0, 0, 0.85)"),
];

impl Theme {
	pub fn light() -> Self {
		Self::dark().with_overrides("light", LIGHT_OVERRIDES)
	}

	pub fn amoled() -> Self {
		Self::dark().with_overrides("amoled", AMOLED_OVERRIDES)
	}

	pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ThemeError> {
		let bytes = fs::read(path.as_ref())?;
		let Value::Object(mut tokens) = serde_json::from_slice(&bytes)? else {
			return Err(ThemeError::NotAnObject);
		};
		let base = match tokens.remove("base") {
			Some(Value::String(name)) => Self::builtin(&name).ok_or(ThemeError::UnknownBase(name))?,
			_ => Self::dark(),
		};
		let name = path
			.as_ref()
			.file_stem()
			.map(|stem| stem.to_string_lossy().into_owned());
		Ok(base.with_file_tokens(name, tokens))
	}

	pub fn builtin(name: &str) -> Option<Self> {
		match name {
			"dark" => Some(Self::dark()),
			"light" => Some(Self::light()),
			"amoled" => Some(Self::amoled()),
			_ => None,
		}
	}

	pub fn system() -> Self {
		match system_theme::current() {
			SystemTheme::Dark => Self::dark(),
			SystemTheme::Light => Self::light(),
		}
	}

	pub fn resolve(name: &str, themes_dir: &Path) -> Self {
		if name == SYSTEM_THEME {
			return Self::system();
		}
		if let Some(theme) = Self::builtin(name) {
			return theme;
		}
		Self::from_file(themes_dir.join(format!("{name}.json"))).unwrap_or_else(|error| {
			tracing::warn!(%error, theme = name, "could not load theme, using dark");
			Self::dark()
		})
	}

	pub fn current(cx: &App) -> Self {
		cx.global::<Self>().clone()
	}

	pub fn install(mut self, cx: &mut App) {
		self.resolve_fonts(&cx.text_system().all_font_names());
		self.scale_fonts(current_font_scale(cx));
		self.ui_scale = current_ui_scale(cx);
		apply_to_components(&self, cx);
		cx.set_global(self);
		cx.refresh_windows();
	}

	pub fn set_font_scale(scale: f32, cx: &mut App) {
		cx.set_global(FontScale(scale));
	}

	pub fn set_ui_scale(scale: f32, cx: &mut App) {
		cx.set_global(UiScale(scale));
		let mut theme = Self::current(cx);
		theme.ui_scale = scale;
		apply_to_components(&theme, cx);
		cx.set_global(theme);
		cx.refresh_windows();
	}

	pub fn rescale_fonts(scale: f32, cx: &mut App) {
		let factor = scale / current_font_scale(cx);
		cx.set_global(FontScale(scale));
		let mut theme = Self::current(cx);
		theme.scale_fonts(factor);
		apply_to_components(&theme, cx);
		cx.set_global(theme);
		cx.refresh_windows();
	}

	fn scale_fonts(&mut self, factor: f32) {
		for size in [
			&mut self.font_tiny,
			&mut self.font_xs,
			&mut self.font_sm,
			&mut self.font_md,
			&mut self.font_lg,
			&mut self.font_xl,
		] {
			*size = *size * factor;
		}
	}

	pub fn pixels(&self, length: Rems) -> gpui::Pixels {
		length.to_pixels(rem_pixels(self.ui_scale))
	}

	pub fn is_light(&self) -> bool {
		self.background_primary.l > 0.5
	}

	fn resolve_fonts(&mut self, installed: &[String]) {
		self.ui_font = pick_family(&self.ui_fonts, installed);
		self.code_font = pick_family(&self.code_fonts, installed);
	}

	fn with_overrides(mut self, name: &str, overrides: &[(&str, &str)]) -> Self {
		self.name = name.to_owned().into();
		for (key, value) in overrides {
			let color = parse_color(value).expect("builtin color override");
			assert!(self.set_color(key, color), "unknown builtin token {key}");
		}
		self
	}

	fn with_file_tokens(mut self, name: Option<String>, tokens: Map<String, Value>) -> Self {
		if let Some(name) = name {
			self.name = name.into();
		}
		for (key, value) in tokens {
			if !self.apply_file_token(&key, &value) {
				tracing::warn!(token = %key, "ignoring unknown or malformed theme token");
			}
		}
		self
	}

	fn apply_file_token(&mut self, key: &str, value: &Value) -> bool {
		match value {
			Value::String(text) => parse_color(text).is_some_and(|color| self.set_color(key, color)),
			Value::Number(number) => number
				.as_f64()
				.is_some_and(|size| self.set_size(key, size as f32)),
			Value::Array(items) => {
				let families: Vec<SharedString> = items
					.iter()
					.filter_map(|item| item.as_str().map(|family| family.to_owned().into()))
					.collect();
				if families.is_empty() {
					return false;
				}
				match key {
					"ui_fonts" => self.ui_fonts = families,
					"code_fonts" => self.code_fonts = families,
					_ => return false,
				}
				true
			}
			_ => false,
		}
	}
}

impl Global for Theme {}

struct FontScale(f32);

impl Global for FontScale {}

struct UiScale(f32);

impl Global for UiScale {}

fn current_ui_scale(cx: &App) -> f32 {
	cx.try_global::<UiScale>().map_or(1.0, |scale| scale.0)
}

fn current_font_scale(cx: &App) -> f32 {
	cx.try_global::<FontScale>().map_or(1.0, |scale| scale.0)
}

fn to_families(names: &[&str]) -> Vec<SharedString> {
	names.iter().map(|name| (*name).to_owned().into()).collect()
}

fn pick_family(families: &[SharedString], installed: &[String]) -> SharedString {
	families
		.iter()
		.find(|family| installed.iter().any(|name| name.eq_ignore_ascii_case(family)))
		.or_else(|| families.last())
		.cloned()
		.unwrap_or_else(|| ".SystemUIFont".into())
}

pub fn parse_color(text: &str) -> Option<Hsla> {
	let text = text.trim();
	if let Some(hex) = text.strip_prefix('#') {
		return parse_hex(hex);
	}
	let body = text
		.strip_prefix("rgba(")
		.or_else(|| text.strip_prefix("rgb("))?
		.strip_suffix(')')?;
	let mut parts = body.split(',').map(str::trim);
	let red: f32 = parts.next()?.parse().ok()?;
	let green: f32 = parts.next()?.parse().ok()?;
	let blue: f32 = parts.next()?.parse().ok()?;
	let alpha: f32 = parts.next().map_or(Some(1.), |alpha| alpha.parse().ok())?;
	Some(
		Rgba {
			r: red / 255.,
			g: green / 255.,
			b: blue / 255.,
			a: alpha,
		}
		.into(),
	)
}

fn parse_hex(hex: &str) -> Option<Hsla> {
	let value = u32::from_str_radix(hex, 16).ok()?;
	let rgba = match hex.len() {
		6 => Rgba {
			r: ((value >> 16) & 0xff) as f32 / 255.,
			g: ((value >> 8) & 0xff) as f32 / 255.,
			b: (value & 0xff) as f32 / 255.,
			a: 1.,
		},
		8 => Rgba {
			r: ((value >> 24) & 0xff) as f32 / 255.,
			g: ((value >> 16) & 0xff) as f32 / 255.,
			b: ((value >> 8) & 0xff) as f32 / 255.,
			a: (value & 0xff) as f32 / 255.,
		},
		_ => return None,
	};
	Some(rgba.into())
}

pub fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
	hsla(color.h, color.s, color.l, alpha)
}

fn apply_to_components(theme: &Theme, cx: &mut App) {
	let mode = if theme.is_light() {
		ThemeMode::Light
	} else {
		ThemeMode::Dark
	};
	ComponentTheme::change(mode, None, cx);
	let colors = component_colors(theme, mode);
	let rem_size = rem_pixels(theme.ui_scale);
	let radius_sm = theme.radius_sm.to_pixels(rem_size);
	let radius_md = theme.radius_md.to_pixels(rem_size);
	let ui_font = theme.ui_font.clone();
	let code_font = theme.code_font.clone();
	let font_size = theme.font_md.to_pixels(rem_size);
	let mono_size = theme.font_xs.to_pixels(rem_size);
	ComponentTheme::update(cx, |component| {
		component.colors = colors;
		component.font_family = ui_font;
		component.mono_font_family = code_font;
		component.font_size = font_size;
		component.mono_font_size = mono_size;
		component.radius = radius_sm;
		component.radius_lg = radius_md;
		component.shadow = false;
	});
}

fn component_colors(theme: &Theme, mode: ThemeMode) -> ThemeColor {
	let mut colors = match mode {
		ThemeMode::Light => *ThemeColor::light(),
		ThemeMode::Dark => *ThemeColor::dark(),
	};
	let transparent = hsla(0., 0., 0., 0.);
	colors.background = theme.background_primary;
	colors.foreground = theme.text_normal;
	colors.border = theme.divider;
	colors.input = theme.interactive_muted;
	colors.muted = theme.background_secondary;
	colors.muted_foreground = theme.text_muted;
	colors.accent = theme.brand;
	colors.accent_foreground = theme.text_on_brand;
	colors.popover = theme.background_floating;
	colors.popover_foreground = theme.text_normal;
	colors.primary = theme.brand;
	colors.primary_hover = theme.brand_hover;
	colors.primary_active = theme.brand_hover;
	colors.primary_foreground = theme.text_on_brand;
	colors.secondary = theme.button_secondary;
	colors.secondary_hover = theme.button_secondary_hover;
	colors.secondary_active = theme.button_secondary_hover;
	colors.secondary_foreground = theme.text_on_brand;
	colors.button = theme.button_secondary;
	colors.button_hover = theme.button_secondary_hover;
	colors.button_active = theme.button_secondary_hover;
	colors.button_foreground = theme.text_on_brand;
	colors.button_primary = theme.brand;
	colors.button_primary_hover = theme.brand_hover;
	colors.button_primary_active = theme.brand_hover;
	colors.button_primary_foreground = theme.text_on_brand;
	colors.button_secondary = theme.button_secondary;
	colors.button_secondary_hover = theme.button_secondary_hover;
	colors.button_secondary_active = theme.button_secondary_hover;
	colors.button_secondary_foreground = theme.text_on_brand;
	colors.button_danger = theme.danger;
	colors.button_danger_hover = theme.danger_hover;
	colors.button_danger_active = theme.danger_hover;
	colors.button_danger_foreground = theme.text_on_brand;
	colors.danger = theme.danger;
	colors.danger_hover = theme.danger_hover;
	colors.danger_active = theme.danger_hover;
	colors.danger_foreground = theme.text_on_brand;
	colors.success = theme.success;
	colors.success_hover = theme.success;
	colors.success_active = theme.success;
	colors.warning = theme.warning;
	colors.warning_hover = theme.warning;
	colors.warning_active = theme.warning;
	colors.link = theme.link;
	colors.link_hover = theme.link;
	colors.link_active = theme.link;
	colors.caret = theme.text_normal;
	colors.ring = theme.brand;
	colors.selection = with_alpha(theme.brand, 0.4);
	colors.overlay = theme.overlay;
	colors.scrollbar = transparent;
	colors.scrollbar_thumb = theme.scrollbar_thumb;
	colors.scrollbar_thumb_hover = theme.scrollbar_thumb;
	colors.list = theme.background_secondary;
	colors.list_even = theme.background_secondary;
	colors.list_hover = theme.modifier_hover;
	colors.list_active = theme.modifier_selected;
	colors.list_active_border = transparent;
	colors.list_head = theme.background_secondary;
	colors.sidebar = theme.background_secondary;
	colors.sidebar_foreground = theme.text_normal;
	colors.sidebar_border = transparent;
	colors.sidebar_accent = theme.modifier_selected;
	colors.sidebar_accent_foreground = theme.interactive_active;
	colors.sidebar_primary = theme.brand;
	colors.sidebar_primary_foreground = theme.text_on_brand;
	colors.title_bar = theme.background_tertiary;
	colors.title_bar_border = transparent;
	colors.window_border = theme.background_tertiary;
	colors.switch = theme.interactive_muted;
	colors.switch_thumb = theme.text_on_brand;
	colors.slider_bar = theme.brand;
	colors.slider_thumb = theme.text_on_brand;
	colors.progress_bar = theme.brand;
	colors.tab = transparent;
	colors.tab_active = theme.modifier_selected;
	colors.tab_active_foreground = theme.interactive_active;
	colors.tab_foreground = theme.interactive_normal;
	colors.tab_bar = transparent;
	colors.skeleton = theme.modifier_selected;
	colors
}
