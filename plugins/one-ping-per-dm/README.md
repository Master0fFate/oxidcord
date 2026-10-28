# One Ping Per DM

The first message from a direct message or group chat plays its sound as usual. Further messages from the same chat are shown silently until you read the chat, then the next message pings again.

Settings:
- `channelToAffect`: `both_dms` (default), `user_dm` for direct messages only or `group_dm` for group chats only.
- `allowMentions`: keep the sound for messages that mention you. Default off.
- `allowEveryone`: keep the sound for @everyone and @here. Default off.

Limits: the silent notification still shows on screen, only the sound is dropped, as the notification hook of Oxidcord offers. Server messages are never touched. A chat counts as read when the client reports it as read, the plugin forgets chats it has not seen read again after a restart.

Permissions: `notifications-filter`, `context-read-state`, `context-private`, `storage`.

Port of the Vencord plugin onePingPerDM.
