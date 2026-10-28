# Quick Mention

Adds an `@` button to the message hover bar. Clicking it puts a mention of the message author into the composer, followed by a space.

Limits: the Vencord plugin hides the button in channels where you cannot send messages. The plugin API has no check for that without extra permissions, so the button always shows.

Permissions: `ui-mount`, `composer`.

Port of the Vencord plugin quickMention.
