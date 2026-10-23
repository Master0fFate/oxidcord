# Quick Replies

A `Snippets` button in the `composer-toolbar` slot opens a `popover` slot with your saved snippets. Pick one to send it to the open channel. Type `name text` into the input and press Enter to save a snippet, use Delete to remove one.

`/snip <name>` sends a snippet to the channel the command was typed in.

Snippets live in plugin storage under `snippet.<name>`. The toolbar needs the open channel, so it does not work in direct messages without `context-private`; `/snip` works everywhere.

Permissions: `ui-slots`, `storage`, `actions-send`, `context-basic` (finds the open channel for the toolbar button).
