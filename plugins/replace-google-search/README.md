# Replace Google Search

Adds a search entry to the menu that opens on selected text and searches the selection in the browser.

- `replacementEngine` off (default): a Search text submenu lists every engine, plus your custom one.
- `replacementEngine` set to an engine: a single Search with <engine> entry.
- `custom`: uses `customEngineName` and `customEngineURL`.

Engines: Google, DuckDuckGo, Brave, Bing, Yahoo, Yandex, Ecosia, GitHub, Reddit, Wikipedia, Startpage, Kagi.

Settings:
- `replacementEngine`: `off` (default), `custom` or an engine name.
- `customEngineName`: label of your engine.
- `customEngineURL`: address the encoded text is appended to, for example `https://google.com/search?q=`. Must start with https:// or http://.

Limits: the client has no built-in Google search entry to replace, so this adds its own. Selections over about 500 characters are cut. The host allows `open-url` once per 10 seconds, so a second search within 10 seconds shows a notice.

Permissions: `ui-mount`, `actions-open-url`, `storage`.

Port of the Vencord plugin replaceGoogleSearch.
