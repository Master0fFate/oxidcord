# Copy Emoji Markdown

Adds Copy emoji to the menu of an emoji, in the picker and inside messages.

- Custom emoji copy as `<:name:id>`, animated ones as `<a:name:id>`. A trailing `~1` style suffix on the name is removed.
- Unicode emoji copy as the character itself.

Settings: `copyUnicode` (default on) allows copying unicode emoji.

Limits: with `copyUnicode` off Vencord copies the `:name:` shortcode of a unicode emoji. The client only gives plugins the character, so this port copies nothing for unicode emoji and shows a notice instead.

Permissions: `ui-mount`, `actions-clipboard`, `storage`.

Port of the Vencord plugin copyEmojiMarkdown.
