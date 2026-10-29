# Favorite Emoji First

Puts your favorite emoji first in the emoji autocomplete, then the emoji you used most recently, then the rest in the usual order.

The client does not read your favorite emoji from your Discord settings yet, so the favorites list is empty for now and fills in as soon as the client supports it. Until then the plugin learns from use: every custom or unicode emoji in a message you send moves to the front of a list of the last 50 emoji, kept in the plugin storage.

Limitations: emoji sent through reactions or the emoji picker are not learned, only emoji that are in the text of a message you send. Unicode emoji are found by their code point ranges, a few rare symbols may be missed.

Permissions: `ui-input`, `context-expressions`, `message-send`, `storage`.

Port of the Vencord plugin favEmojiFirst.
