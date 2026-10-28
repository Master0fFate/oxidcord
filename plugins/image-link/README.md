# Image Link

When a message is nothing but a link to an image or gif, Oxidcord shows only the image and drops the link text. This plugin adds the link back as a clickable line under the image.

Limits: the message list decides to hide the link text before plugins run and `render-message` cannot change that, so the link comes back as a line under the message instead of in its original place. With link previews turned off in Oxidcord the text is not hidden and the link shows twice. Video links and messages in direct messages are not touched (no `context-private`).

Permissions: `message-intercept`.

Port of the Vencord plugin imageLink.
