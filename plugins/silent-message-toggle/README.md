# Silent Message Toggle

Adds a bell button to the composer toolbar. While it is on, the next message is sent silently, without a notification for the people in the channel. The client sends the silent flag itself, the message text is not changed.

Settings: `persistState` (default off) keeps the toggle on when you change channels, otherwise it turns off on a channel switch. `autoDisable` (default on) turns it off again after a silent message was sent.

Limitations: the toggle is one switch for the whole client, not one per composer.

Permissions: `message-send`, `ui-mount`, `context-basic`, `storage`.

Port of the Vencord plugin silentMessageToggle.
