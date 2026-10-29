# No Server Emojis

Keeps server emoji out of the emoji autocomplete. `shownEmojis` picks what stays: `onlyUnicode` (default) shows only unicode emoji, `currentServer` also shows the emoji of the server you are typing in, `all` is the client default.

Limitations: the filter works on the suggestions the client offers for the typed query, up to 200 of them. When a query matches only server emoji nothing is left to show, the plugin then has no answer and the client shows its usual suggestions. In direct messages `currentServer` hides every server emoji, and without the `context-private` permission the plugin does not know it is in a direct message.

Permissions: `ui-input`, `context-basic`, `storage`.

Port of the Vencord plugin noServerEmojis.
