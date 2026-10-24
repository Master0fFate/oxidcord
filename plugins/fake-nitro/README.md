# Fake Nitro

Lets you post custom emoji your account cannot use, the way people do it without Nitro: as an image link to the emoji on Discord's CDN. Received emoji links are shown inline.

What it does:

- In the emoji picker, emoji you cannot use become clickable. Picking one puts `https://cdn.discordapp.com/emojis/<id>.<webp|gif>?size=48&name=<name>` into the composer.
- Before a message is sent, every `<:name:id>` or `<a:name:id>` you typed for an emoji you cannot use in that channel is replaced by the same link. Emoji you can use are left alone. The message shows the "edited by plugin" marker.
- In received messages, such links are drawn as an inline emoji, their link preview is hidden, and a line under the message says the links were shown inline by Fake Nitro.

What it cannot do:

- The messages you send contain links, not emoji. Everyone else sees a link and Discord's image preview, unless they also run a client that shows those links inline.
- It cannot add reactions with emoji you cannot use. Reactions go through Discord and require the real permission.
- It does not unlock stickers, animated avatars, profile banners, larger uploads or any other Nitro feature, and nothing on Discord's side changes.
- Emoji in direct messages are not shown inline, because the plugin does not ask for `context-private`.
- In the client, a pick from a thread composer lands in the composer that last asked a plugin for an emoji.

Permissions: `composer` (puts the link into the message box when you pick an emoji), `context-expressions` (checks which emoji you can use), `message-send` (rewrites typed emoji before sending), `message-intercept` (shows received links inline).

Plugin API 1.1, built with the SDK feature `ext`.
