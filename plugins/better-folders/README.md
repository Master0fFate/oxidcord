# Better Folders

Shows the servers of an open folder in a dedicated sidebar next to the server list.

Settings:
- `sidebar`: turn the folder sidebar on. Default on.
- `keepIcons`: keep the small server icons on a folder in the server list while it is open in the sidebar. With it off the folder shows as a plain folder icon. Default off.
- `sidebarAnim`, `closeAllFolders`, `closeAllHomeButton`, `closeOthers`, `closeServerFolder`, `forceOpen`, `showFolderIcon`: kept so the settings keep their place, they do nothing.

Limitations: partial port. Plugins cannot read or change which folders are open, so the close and force open rules cannot be driven. The sidebar has no folder icon above the servers and does not animate, and `showFolderIcon` is a word (`never`, `always`, `multiple`) instead of Vencord's number. `keepIcons` is the inverse of the client's plain folder icons option, so it also overrides Plain Folder Icon while the sidebar is on. The client reverts both options when the plugin is turned off.

Permissions: `actions-settings`, `storage`.

Port of the Vencord plugin betterFolders.
