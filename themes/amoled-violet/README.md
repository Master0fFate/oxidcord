# Amoled Violet

It starts from the built-in `amoled` theme and changes three things.

- True black backgrounds with violet tinted secondary surfaces, dividers and inputs, so panels separate without grey.
- A violet accent (`#8b5cf6`) for the brand color, links, mentions, unread pills and the speaking ring.
- Rounder corners (`radius_sm` 6, `radius_md` 12, `radius_lg` 20, `radius_xl` 28) and slightly taller fields, controls and chips.

Install it from Settings, Themes, catalog section, or copy `theme.json` into your themes directory. See `docs/themes.md` for the token list.

It also ships Montserrat (SIL Open Font License, see `fonts/OFL.txt`) as the interface font. The `font_files` list in `theme.json` names each file with its url and sha256, and installing the theme from the catalog downloads them into `<themes dir>/fonts`. Without the fonts the theme falls back to the next family in `ui_fonts`.
