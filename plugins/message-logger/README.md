# Message Logger

Replaces Edit History and Delete Keeper.

Deleted messages stay in the chat, shown in red with a `(deleted)` marker. The host keeps the deleted message in its cache and asks the plugin to render it again with the deleted flag set; the plugin keeps every one it is asked about. They are kept in memory only and are gone after a restart, the same as a reload in other clients.

Edited messages keep the `(edited)` marker the client already shows, and get a `N earlier versions` button underneath. Clicking it lists the original and every edit, clicking `Hide history` folds it again. Up to 10 versions per message and the 200 most recently edited messages are kept, each version cut to 2000 characters and shown cut to 300.

The plugin remembers the text of the last 1000 messages it renders or receives events for, so it can notice when one changes. Edits to messages it never saw have no earlier text to keep.

Storage: one `edits.<id>` key per edited message, so the history survives restarts.

Direct messages are not logged: the plugin does not request `context-private`.

Permissions: `context-timeline`, `message-render`, `storage`, `ui-slots`.
