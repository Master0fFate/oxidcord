use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gpui::{App, Global, Hsla, Rems, Rgba, SharedString, hsla, px, rems};
use gpui_component::{Theme as ComponentTheme, ThemeColor, ThemeMode};
use platform::system_theme::{self, SystemTheme};
use serde_json::{Map, Value};

pub const SYSTEM_THEME: &str = "system";
const BASE_REM: f32 = 16.;
const FONTS_FOLDER: &str = "fonts";
const MAX_FONT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FONT_FILES: usize = 8;

pub fn rem(pixels: f32) -> Rems {
	rems(pixels / BASE_REM)
}

pub fn rem_to_pixels(measure: Rems) -> f32 {
	measure.0 * BASE_REM
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
			pub font_files: Vec<PathBuf>,
			$(pub $color: Hsla,)+
			$(pub $size: Rems,)+
			pub ui_scale: f32,
			pub roundness: f32,
		}

		impl Theme {
			pub fn dark() -> Self {
				let ui_fonts = to_families(UI_FONTS);
				let code_fonts = to_families(CODE_FONTS);
				Self {
					name: "dark".into(),
					ui_font: ui_fonts[0].clone(),
					code_font: code_fonts[0].clone(),
					font_files: Vec::new(),
					ui_fonts,
					code_fonts,
					$($color: parse_color($default).expect("builtin color token"),)+
					$($size: rem($value),)+
					ui_scale: 1.,
					roundness: 1.,
				}
			}

			fn set_color(&mut self, key: &str, value: Hsla) -> bool {
				match key {
					$(stringify!($color) => self.$color = value,)+
					_ => return false,
				}
				true
			}

			fn named_color(&self, key: &str) -> Option<Hsla> {
				match key {
					$(stringify!($color) => Some(self.$color),)+
					_ => None,
				}
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
		background_primary = "#1a1a1e",
		background_secondary = "#202024",
		background_tertiary = "#121214",
		background_floating = "#28282d",
		user_panel_background = "#202024",
		modifier_hover = "rgba(151, 151, 159, 0.08)",
		modifier_selected = "rgba(151, 151, 159, 0.16)",
		modifier_active = "rgba(151, 151, 159, 0.24)",
		text_normal = "#dfe0e2",
		text_muted = "#94959c",
		header_primary = "#fbfbfb",
		header_secondary = "#c7c8ce",
		interactive_normal = "#b5b6bd",
		interactive_hover = "#dfe0e2",
		interactive_active = "#ffffff",
		interactive_muted = "#4e4f56",
		brand = "#5865f2",
		brand_hover = "#4752c4",
		text_on_brand = "#ffffff",
		status_online = "#3d9e60",
		status_idle = "#ffc04e",
		status_dnd = "#da3e44",
		status_offline = "#82838b",
		status_streaming = "#593695",
		mention_background = "rgba(88, 101, 242, 0.24)",
		mention_text = "#c9cdfb",
		link = "#00a8fc",
		danger = "#da3e44",
		danger_hover = "#c2363c",
		success = "#3d9e60",
		warning = "#ffc04e",
		spoiler = "#29292d",
		code_background = "#1f1f23",
		code_border = "#2c2c30",
		scrollbar_thumb = "#3a3a40",
		scrollbar_track = "#1a1a1e",
		divider = "rgba(255, 255, 255, 0.07)",
		input_background = "#222327",
		button_secondary = "#2c2c30",
		button_secondary_hover = "#36363c",
		unread_pill = "#f2f3f5",
		mention_badge = "#d22d39",
		overlay = "rgba(0, 0, 0, 0.7)",
		speaking_ring = "#3d9e60",
		live_badge = "#d22d39",
	}
	sizes {
		title_bar_height = 30.,
		server_rail_width = 72.,
		server_icon = 40.,
		channel_sidebar_width = 240.,
		member_list_width = 240.,
		top_bar_height = 48.,
		user_panel_height = 56.,
		guild_header_height = 48.,
		channel_row_height = 34.,
		member_row_height = 44.,
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
	("background_secondary", "#f8f8f9"),
	("background_tertiary", "#f0f0f2"),
	("background_floating", "#ffffff"),
	("user_panel_background", "#ffffff"),
	("modifier_hover", "rgba(104, 104, 112, 0.08)"),
	("modifier_selected", "rgba(104, 104, 112, 0.14)"),
	("modifier_active", "rgba(104, 104, 112, 0.2)"),
	("text_normal", "#2e2f34"),
	("text_muted", "#5c5d66"),
	("header_primary", "#060607"),
	("header_secondary", "#4e4f58"),
	("interactive_normal", "#4e4f58"),
	("interactive_hover", "#2e2f34"),
	("interactive_active", "#060607"),
	("interactive_muted", "#a4a5ab"),
	("mention_background", "rgba(88, 101, 242, 0.15)"),
	("mention_text", "#3c45a5"),
	("link", "#006ce7"),
	("spoiler", "#d9d9de"),
	("code_background", "#f3f3f4"),
	("code_border", "#e4e4e7"),
	("scrollbar_thumb", "#c4c5ca"),
	("scrollbar_track", "#f3f3f4"),
	("divider", "rgba(0, 0, 0, 0.08)"),
	("input_background", "#f8f8f9"),
	("button_secondary", "#e6e6e9"),
	("button_secondary_hover", "#dcdce0"),
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
		let font_files = font_paths(path.as_ref(), tokens.remove("font_files"));
		let name = path
			.as_ref()
			.file_stem()
			.map(|stem| stem.to_string_lossy().into_owned());
		let mut theme = base.with_file_tokens(name, tokens);
		theme.font_files = font_files;
		Ok(theme)
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
		load_font_files(&self.font_files, cx);
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

	pub fn token(&self, kebab: &str) -> Option<Hsla> {
		self.named_color(&kebab.replace('-', "_"))
	}

	pub fn pixels(&self, length: Rems) -> gpui::Pixels {
		length.to_pixels(rem_pixels(self.ui_scale))
	}

	pub fn round(&self, extent: Rems) -> Rems {
		extent * (self.roundness / 2.)
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
			Value::Number(number) => number.as_f64().is_some_and(|value| {
				if key == "roundness" {
					self.roundness = (value as f32).clamp(0., 1.);
					true
				} else {
					self.set_size(key, value as f32)
				}
			}),
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

struct LoadedFonts(HashSet<PathBuf>);

impl Global for LoadedFonts {}

fn font_paths(theme_file: &Path, declared: Option<Value>) -> Vec<PathBuf> {
	let Some(Value::Array(items)) = declared else {
		return Vec::new();
	};
	let folder = theme_file.with_file_name(FONTS_FOLDER);
	items
		.iter()
		.filter_map(|item| item.get("file")?.as_str())
		.filter(|file| Path::new(file).file_name().is_some_and(|name| name == *file))
		.map(|file| folder.join(file))
		.take(MAX_FONT_FILES)
		.collect()
}

fn load_font_files(paths: &[PathBuf], cx: &mut App) {
	let mut loaded = cx
		.try_global::<LoadedFonts>()
		.map_or_else(HashSet::new, |fonts| fonts.0.clone());
	let fresh: Vec<&PathBuf> = paths.iter().filter(|path| !loaded.contains(*path)).collect();
	let mut fonts = Vec::with_capacity(fresh.len());
	for path in fresh {
		let readable = fs::metadata(path).is_ok_and(|meta| meta.len() <= MAX_FONT_BYTES);
		match fs::read(path).ok().filter(|_| readable) {
			Some(bytes) => {
				loaded.insert(path.clone());
				fonts.push(Cow::Owned(bytes));
			}
			None => tracing::warn!(font = %path.display(), "could not read theme font"),
		}
	}
	if fonts.is_empty() {
		return;
	}
	if let Err(error) = cx.text_system().add_fonts(fonts) {
		tracing::warn!(%error, "could not register theme fonts");
		return;
	}
	cx.set_global(LoadedFonts(loaded));
}

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
	colors.accent = theme.modifier_selected;
	colors.accent_foreground = theme.interactive_active;
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
