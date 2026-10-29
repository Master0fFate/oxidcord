# Voice Chat Narrator

Says out loud who joins, leaves or moves in your voice call, and when you mute, unmute, deafen or undeafen yourself.

Settings (same keys as Vencord):
- `volume`: 0 to 1. Default 1.
- `rate`: 0.5 to 2. Default 1.
- `sayOwnName`: say your own name in the announcements. Default off, your own lines then start with the verb.
- `latinOnly`: strip characters outside the Latin script from names before saying them. Default off.
- `joinMessage`, `leaveMessage`, `moveMessage`, `muteMessage`, `unmuteMessage`, `deafenMessage`, `undeafenMessage`: templates. `{{USER}}`, `{{DISPLAY_NAME}}` and `{{NICKNAME}}` become the display name of the person, `{{CHANNEL}}` the name of the voice channel.

Limitations: partial port.
- Speech works on Windows only today, other systems answer `unavailable` and nothing is said.
- Plugins can say one line per two seconds, announcements that arrive together are joined into one sentence and cut at 300 characters, a line right after another one is dropped.
- Only your own mute and deafen state is known, like Vencord's own-only messages. Someone moving to another channel is announced as leaving.
- Names are display names, Vencord says the username for `{{USER}}`. Names are cleaned without Unicode normalisation, symbols and emoji are dropped.
- Voice selection is not available, the system voice is used. Rate is limited to 0.5 to 2 by the client.
- Leaving a call sends no event, so while you are in a call a check runs once a second to notice it and say your leave line.

Permissions: `context-voice`, `context-members`, `context-basic`, `actions-speak`, `storage`.

Port of the Vencord plugin vcNarrator.
