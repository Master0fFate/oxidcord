# Typing Indicator

Shows a badge on a channel in the channel list, and on direct messages, while someone is typing there. The tooltip names who is typing. The badge goes away ten seconds after the last typing event, or as soon as the person sends their message.

Settings:
- `includeCurrentChannel`: also show it for the channel you are in. Default on.
- `includeMutedChannels`: also show it for muted channels. Default off.
- `includeBlockedUsers`: also show it for users you blocked. Default off.
- `indicatorMode`: `icon` shows a dots icon, `count` shows the icon with the number of people typing. Default `icon`.

Limits: a channel row can only carry an icon with an optional short label, so the Vencord modes with animated dots and small avatars are reduced to `icon` and `count`. There is no ignore state in Oxidcord, so `includeIgnoredUsers` is not part of this port. Threads are not decorated.

Permissions: `ui-input`, `context-timeline`, `context-private`, `context-read-state`, `context-relationships`, `context-basic`, `context-members`, `storage`.

Port of the Vencord plugin typingIndicator.
