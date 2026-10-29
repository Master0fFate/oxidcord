# Expression Cloner

Adds Clone to server to the menu of an emoji and of a sticker. It opens a window with a preview, a server list and, for emoji, a name field. Pressing Clone adds the emoji or sticker to the chosen server.

The servers in the list are the ones you own and the ones where your permissions include Administrator or Create Expressions. The client downloads the image and uploads it, and checks the permission again before doing so. If the server has no free slot or the image is too large, the client shows the error from Discord.

Emoji names are 2 to 32 letters, digits or underscores, a trailing `~1` style suffix is removed from the default name. Stickers keep their own name and tags.

Limits: no settings. Only custom emoji can be cloned, and only stickers the client has already loaded. Servers with no free emoji or sticker slot are still listed because slot counts are not available to plugins, so Discord rejects the request in that case. The permission check reads the first channel of each server, which is enough for server wide permissions but is a best effort.

Permissions: `ui-mount`, `context-directory`, `context-roles`, `context-expressions`, `actions-guild`.

Port of the Vencord plugin expressionCloner.
