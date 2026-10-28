# Role Color Everywhere

Adds the color of the top role of a member to names and mentions wherever the client can draw it.

- Names: members with a role color get that color as a name override, so it also shows in places that do not color names by themselves (reply quotes, typing line, direct message rows).
- Mentions: user mentions in messages get the role color of the mentioned member, with the name as text.
- Message tint: with `colorChatMessages` on, the stripe next to a message takes the role color of its author.

Settings: `chatMentions` (default on), `memberList`, `voiceUsers`, `reactorsList`, `pollResults` (default on), `colorChatMessages` (default off) and `messageSaturation` (0 to 100, default 30, the strength of the message stripe).

Limits:
- `memberList`, `voiceUsers`, `reactorsList` and `pollResults` cannot be controlled one by one. The client applies a name override to all of those places together, so names are colored while any of the four settings is on and left alone when all four are off.
- Vencord tints the text of the whole message. This port tints the accent stripe of the message with `messageSaturation` as its strength, the text color is not changed.
- A colored mention is plain styled text, so it no longer opens the profile when clicked. Mentions of members without a role color stay as they are.
- Members the client has not loaded have no role color to read.

Permissions: `user-display`, `message-intercept`, `context-roles`, `storage`.

Port of the Vencord plugin roleColorEverywhere.
