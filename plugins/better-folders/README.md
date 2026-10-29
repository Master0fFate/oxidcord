# Better Folders

Shows the servers of an open folder in a dedicated sidebar next to the server list.

Settings:
- `sidebar`: turn the folder sidebar on. Default on.
- `keepIcons`: keep the small server icons on a folder in the server list while it is open in the sidebar. With it off the folder shows as a plain folder icon. Default off.

Limitations: plugins cannot read or change which folders are open, so Vencord's close and force open rules are not part of this port, and neither are the sidebar animation and the folder icon above the servers. `keepIcons` is the inverse of the client's plain folder icons option, so it also overrides Plain Folder Icon while the sidebar is on. The client reverts both options when the plugin is turned off.

Permissions: `actions-settings`, `storage`.

Port of the Vencord plugin betterFolders.
