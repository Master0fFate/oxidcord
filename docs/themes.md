# Themes

A theme is one JSON file of overrides on top of a built-in theme. There is no code and nothing runs, so a theme cannot do anything except change how Oxidcord looks.

## Quick start

1. Open Settings, Themes, and use the folder button to open the themes directory.
2. Drop a `.json` file in it. The file name without `.json` is the theme name.
3. Come back to Settings, Themes. The theme is listed with a color swatch. Click it to apply.

The themes directory is `<data dir>/themes`, see the README for where that is on your system. `--data-dir` moves it.

## File format

```json
{
	"base": "amoled",
	"background_primary": "#000000",
	"brand": "#8b5cf6",
	"mention_background": "rgba(139, 92, 246, 0.28)",
	"radius_md": 12,
	"ui_fonts": ["Inter", "Segoe UI", "sans-serif"]
}
```

- `base` picks the theme to start from: `dark` (default), `light` or `amoled`. Every token you leave out keeps the value of the base.
- Colors are `#rrggbb`, `#rrggbbaa`, `rgb(r, g, b)` or `rgba(r, g, b, a)` with `a` from 0 to 1.
- Sizes are plain numbers in pixels at 100% interface scale. The interface zoom setting scales them.
- `ui_fonts` and `code_fonts` are lists of font family names, tried in order.
- Unknown or malformed tokens are ignored and logged, they never stop the theme from loading. An unknown `base` does.

## Color tokens

| Group | Tokens |
| --- | --- |
| Surfaces | `background_primary` (main area), `background_secondary` (sidebars and panels), `background_tertiary` (deepest layer), `background_floating` (menus and popovers), `user_panel_background` |
| Interaction | `modifier_hover`, `modifier_selected`, `modifier_active`, `interactive_normal`, `interactive_hover`, `interactive_active`, `interactive_muted` |
| Text | `text_normal`, `text_muted`, `header_primary`, `header_secondary`, `text_on_brand` |
| Accent | `brand`, `brand_hover`, `link`, `mention_background`, `mention_text`, `unread_pill`, `mention_badge`, `speaking_ring`, `live_badge` |
| Status | `status_online`, `status_idle`, `status_dnd`, `status_offline`, `status_streaming`, `danger`, `danger_hover`, `success`, `warning` |
| Controls | `input_background`, `button_secondary`, `button_secondary_hover`, `divider`, `overlay` |
| Code and spoilers | `spoiler`, `code_background`, `code_border`, `scrollbar_thumb`, `scrollbar_track` |

## Size tokens

The ones worth changing:

| Tokens | What they do |
| --- | --- |
| `radius_sm`, `radius_md`, `radius_lg`, `radius_xl` | Corner radius from small (rows, chips) to large (modals, cards). Defaults 4, 8, 16, 24 |
| `space_2` to `space_32` | The spacing scale used for gaps and padding |
| `font_tiny` to `font_xl` | Text sizes |
| `field_height`, `control_height`, `chip_height` | Height of inputs, buttons and chips |
| `channel_row_height`, `member_row_height`, `compact_row` | Density of the sidebars and compact messages |
| `message_avatar`, `message_gutter` | Message avatar size and the space around the message column |
| `server_rail_width`, `channel_sidebar_width`, `member_list_width` | Column widths |

The complete list, with the defaults, is the `tokens!` block in `crates/ui/src/theme.rs`. Every identifier there is a valid key.

## Ship a theme as a plugin

A theme-only plugin is a folder with a `plugin.toml` without `entry`, and a `theme.json`. It has no permissions and no wasm. Installing it from the catalog or from a `.oxidcord-plugin` file makes the theme appear in Settings, Themes while the plugin is enabled.

```toml
id = "dev.example.theme-sunset"
name = "Sunset"
version = "0.1.0"
api = "^1.0"
authors = ["Example"]
license = "MIT"
description = "Warm dark theme."
min_host = "0.1.0"
```

Pack it with `cargo xtask plugin pack <folder>`. See `docs/plugin-api/publishing.md` for the catalog. The folders `plugins/theme-midnight` and `plugins/theme-amoled-violet` are complete examples.

## Tips

- Start from `amoled` for a black theme and from `dark` otherwise. Overriding five or six tokens is usually enough.
- Keep `text_normal` against `background_primary` above a 7:1 contrast ratio, and `text_muted` above 4.5:1.
- Raise the `radius_*` tokens together. A mix of tight and round corners looks accidental.
- Tint your greys toward the accent (a violet theme uses `#07050c`, not `#080808`), so the surfaces feel related to the brand color.
