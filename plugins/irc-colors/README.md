# IRC Colors

Gives every user a name color of their own, derived from their id, so people are easier to tell apart in busy channels. The color is `hsl(hue, 100%, lightness)` with a hue from 0 to 359.

Settings:
- `lightness`: lightness in percent, default 70.
- `applyColorOnlyToUsersWithoutColor`: keep the role color of members that have one. Needs the `context-roles` permission, without it every member counts as having no role color.
- `applyColorOnlyInDms`: only color names in direct messages and leave servers alone.

Limits: Vencord has a `memberListColors` setting to leave the member list alone. The client applies a name override to messages, the member list and the profile popout together, so the setting is not part of this port and the member list is always colored. Role colors are replaced by the generated color, the role dot setting of Oxidcord still shows them.

Permissions: `user-display`, `storage`, `context-roles`.

Port of the Vencord plugin ircColors.
