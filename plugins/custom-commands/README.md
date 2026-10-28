# Custom Commands

Turns saved text into slash commands, called tags.

- `/tags create <name> <text>` saves a tag. The name uses letters, digits, `-` and `_` (up to 32 characters). Saving an existing name replaces it.
- `/tags list` opens a dialog with your tags and a delete button for each.
- `/tags delete <name>` removes a tag.
- `/<name>` sends the text of the tag in the current channel.

Tag text can hold arguments in double curly braces: `Hello {{user}}, I am {{mood = great}}`. A default after `=` is used when no value is given. Values come from what you type after the command, one word per argument in the order they first appear, the last argument takes the rest of the line (`/greet Clyde very happy`). Write `\n` in the text for a line break. At most 100 tags of 2000 characters.

Limits: the plugin API has no slash command options, so arguments are positional words instead of named fields, and there is no `ephemeral` option because plugins cannot show local-only messages. A command cannot be unregistered while the plugin runs: a deleted tag stays in the slash list, described as deleted, until the plugin restarts, and only shows a note when used. Pending replies are not attached to the tag message.

Permissions: `ui-slots`, `ui-mount`, `storage`, `actions-send`.

Port of the Vencord plugin customCommands.
