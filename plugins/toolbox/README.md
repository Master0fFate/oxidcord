# Toolbox

Adds a wrench button to the title bar. It opens a popover with quick actions: open the plugin settings, copy the client version, and a list of the plugins that are turned on with their version and an ext badge for the ones that use the extended API.

Settings:
- `showPluginMenu`: show the list of plugins in the popover. Default on.

Limitations: plugins cannot be turned on or off from here, the client does not allow it. The list shows the first 100 plugins by name, and it is read when the plugin starts and again with its Refresh button. Vencord's quick actions for its own settings, notifications and developer tools have no counterpart.

Permissions: `ui-mount`, `plugins-list`, `context-basic`, `actions-navigate`, `actions-clipboard`, `storage`.

Port of the Vencord plugin vencordToolbox.
