# Better Role Dot

Adds a small dot in the color of the member's top colored role next to their name. The tooltip names the role and a click on the dot copies the color as `#rrggbb`.

The dot shows in messages, the member list and the profile popout. It is only drawn in servers, for members that have at least one colored role.

Settings:
- `bothStyles`: also set the name itself to the role color. Default off, the client keeps its own name colors.
- `copyRoleColorInProfilePopout`: accepted so the setting keeps its place, it has no separate effect. Clicking the dot copies the color wherever the dot shows, the popout included.

Limits: the dot is a badge of the client, so its look follows the badge style and not Discord's role dot. A user who has different colors in several servers copies the color of the server drawn last. The role list is read again every 30 seconds. Only one display plugin can set a name color, the first plugin in priority order wins.

Permissions: `user-display`, `context-roles`, `actions-clipboard`, `ui-mount`, `storage`.

Port of the Vencord plugin betterRoleDot.
