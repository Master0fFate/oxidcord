# Copy Sticker Links

Adds two entries to the menu of a sticker, in the picker and under a message.

- Copy link puts the sticker image link (512 px) on the clipboard.
- Open link opens it in the browser.

The link format follows the sticker format: png for PNG and APNG, gif for GIF, json for Lottie.

Limits: no settings. A sticker the client does not know (from a server you are not in) is treated as png, which is right for most stickers. Open link uses `open-url`, which the host limits to one call per 10 seconds.

Permissions: `ui-mount`, `context-expressions`, `actions-clipboard`, `actions-open-url`.

Port of the Vencord plugin copyStickerLinks.
