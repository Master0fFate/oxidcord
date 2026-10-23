# Word Mute

Hides messages that contain a word or phrase you muted. Your own messages are never hidden.

Manage the list from the `Word Mute` page in the plugin settings: type a word or phrase and press Enter to add it, use Remove to drop one, Clear all to empty the list. The same works from the composer:

- `/mute <word or phrase>`
- `/unmute <word or phrase>`
- `/mutes` refreshes the settings page
- `/unmuteall`

Matching is case-insensitive and by whole word unless the `Match whole words only` setting is off, in which case any substring matches. A phrase matches as written, so `season finale` hides messages that contain those two words in a row.

The list holds up to 200 terms of at most 64 characters, kept in plugin storage under `terms`. Terms are lowercased once when the plugin starts or the list changes, so the filter does one lowercase pass per message.

Direct messages are not filtered because that needs `context-private`, which this plugin does not ask for. Settings page buttons refilter cached messages at once. Slash commands change the list immediately but cached messages are filtered again on the next render epoch change.

Permissions: `message-filter`, `storage`, `ui-slots`, `context-basic` (finds your own user id).
