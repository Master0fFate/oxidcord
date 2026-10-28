# Hide Attachments

Adds an eye button to the message hover bar. Clicking it hides every attachment and embed of that message and puts a `Media hidden` line under it, clicking it again shows them. The messages you hid are saved (the newest 2000) and stay hidden after a restart.

Limits: stickers stay visible, the plugin API cannot hide them. The button is the same on every message, so it has no separate show and hide labels. Messages in direct messages are not touched because the plugin does not ask for `context-private`.

Permissions: `ui-mount`, `message-intercept`, `storage`.

Port of the Vencord plugin hideAttachments.
