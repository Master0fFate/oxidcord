# Code Highlight

Syntax highlighting for fenced code blocks that name a language, with a small tokenizer built into the plugin.

Languages (fence names): `rust`/`rs`, `javascript`/`js`/`jsx`, `typescript`/`ts`/`tsx`, `python`/`py`, `go`, `c`/`h`, `cpp`/`c++`/`cc`/`hpp`, `java`, `cs`/`csharp`, `json`, `toml`, `yaml`/`yml`, `sh`/`bash`/`zsh`, `html`/`xml`/`svg`, `css`/`scss`, `sql`. A block without a language, or with another one, keeps the plain look.

The tokenizer is one pass over the text and recognises keywords, types, function calls, strings, numbers, comments, punctuation, properties (JSON, YAML, TOML and CSS keys, HTML attributes) and variables (shell). It does not parse, so it can be wrong on tricky syntax (regex literals, nested templates, raw strings). Blocks over 16 KB are left alone.

Settings: `theme` (`dark-plus` by default, `one-dark`, `monokai`, `github-light`).

Limits: Vencord uses Shiki with the full VS Code grammars and themes, custom theme links, language icons and the "try Discord's highlighter" choice. None of that is part of this port. The palettes are fixed colors, pick `github-light` only with a light theme.

Permissions: `message-intercept`, `storage`.

Port of the Vencord plugin shikiCodeblocks.
