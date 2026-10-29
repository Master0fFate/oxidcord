# Better Role Context

Adds three entries to the menu of a role chip in the profile popout.

- Copy color: puts the role color on the clipboard as `#rrggbb`. Roles without a color show a notice.
- Copy role id.
- View icon: opens a window with the role icon and an Open link to the large version. Roles without an icon show a notice.

Settings: `roleIconFileFormat`, `png` (default), `webp` or `jpg`.

Limits: Edit Role and View Role Members of Vencord are dropped, plugins cannot open the server settings or the member list of a role. The entries show on every role, a role without a color or icon answers with a notice.

Permissions: `ui-mount`, `context-roles`, `actions-clipboard`, `storage`.

Port of the Vencord plugin betterRoleContext.
