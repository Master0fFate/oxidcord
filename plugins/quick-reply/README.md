# Quick Reply

Reply to messages with Ctrl+Up and Ctrl+Down and edit your own with Ctrl+Shift+Up and Ctrl+Shift+Down. The first press picks the newest message, Up moves to an older one and Down to a newer one. Moving past the newest message stops the stepping, the next Up starts again from the newest. Escape is handled by the client like for any reply or edit.

Settings: `shouldMention` is `plugin` (default), `enabled` or `disabled`. `plugin` starts the reply with the ping on and leaves the decision to the No Reply Mention plugin, which sets the ping when the message is sent; without that plugin the reply pings. `ignoreBlockedAndIgnored` (default on) skips messages of blocked users.

Limitations: the steps are taken over the last 100 messages the client has loaded for the channel. Direct messages are not covered, the plugin cannot see them without the `context-private` permission. Ignored users do not exist in the client, only blocked users are skipped. Messages the client cannot reply to, like system messages, are not told apart. The Ctrl+Up and Ctrl+Down chords no longer move the cursor in the composer while the plugin is on.

Permissions: `ui-keybinds`, `context-basic`, `context-timeline`, `context-relationships`, `actions-compose`, `storage`.

Port of the Vencord plugin quickReply.
