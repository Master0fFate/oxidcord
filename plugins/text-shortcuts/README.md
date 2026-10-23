# Text Shortcuts

Expands shortcuts in a message before it is sent. The composer shows its usual indicator that a plugin changed the message.

Built in: `:shrug:`, `:lenny:` (group `faces`) and `:tableflip:`, `:unflip:` (group `flip`).

Your own shortcuts start with `;`, for example `;brb` for `be right back`. Add them from the Shortcuts button in the composer toolbar (type `name text` and press Enter) or with `/shortcut add <name> <text>`, `/shortcut remove <name>` and `/shortcut list`.

A shortcut only expands as a whole word: at the start of the message, after whitespace or after `(`, and before whitespace, the end of the message or one of `.,!?)`. It is left alone inside inline code, code blocks and tokens that contain `://`. Put a backslash in front (`\:shrug:`) to send the literal text without the backslash.

Settings: switch `faces`, `flip` and your own shortcuts on or off.

Limits: 100 shortcuts, names of 1 to 32 characters from `a-z 0-9 _ -`, replacements up to 500 characters. A message that would end up longer than 2000 characters is sent unchanged. The plugin never cancels a message. Expansion is a single pass, so a replacement containing another shortcut is not expanded again.

The `s/old/new/` correction is not included: the plugin API cannot edit a sent message.

Permissions: `message-send`, `storage`, `ui-slots`.
