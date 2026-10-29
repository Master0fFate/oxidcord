# Better Role Dot

Adds a small colored badge (a circle icon) in the color of the member's top colored role next to their name. The tooltip names the role and a click on the badge copies the color as `#rrggbb`.

The badge shows in messages, the member list and the profile popout. It is only drawn in servers, for members that have at least one colored role.

Settings:
- `bothStyles`: also set the name itself to the role color. Default off, the client keeps its own name colors.

Limits: the badge follows the badge style of the client, it is not Discord's role dot. A user who has different colors in several servers copies the color of the server drawn last. The role list is read again every 30 seconds. Only one display plugin can set a name color, the first plugin in priority order wins.

Permissions: `user-display`, `context-roles`, `actions-clipboard`, `ui-mount`, `storage`.

Port of the Vencord plugin betterRoleDot.
