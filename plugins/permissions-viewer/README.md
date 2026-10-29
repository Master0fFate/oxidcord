# Permissions Viewer

Shows permissions and roles in a window.

- Member menu, View permissions (servers only): two tabs. Permissions lists every permission of the member in a channel with a check or a cross. Roles lists the roles of the member. The channel is the one you have open if it belongs to that server, otherwise the first channel of the server.
- Channel menu, View permissions: your own permissions in that channel.
- Server menu, View roles: every role with its color and position.

Settings: `permissionsSortOrder`, `highest` (default) or `lowest` role first. It orders the roles in the Roles tab and in View roles.

Limits: permissions are the final result for the person in the channel, as the client computes them with roles and channel overrides. Vencord also shows which role or override grants each permission and lists the overrides of a channel, plugins only get the final bits. The View permissions button in the profile popout is not added. Names of permissions follow the Discord permission list.

Permissions: `ui-mount`, `context-roles`, `context-directory`, `context-basic`, `storage`.

Port of the Vencord plugin permissionsViewer.
