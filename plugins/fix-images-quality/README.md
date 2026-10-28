# Fix Images Quality

Loads image attachments from the original file instead of the downscaled copy the client picks for the chat column, so they stay sharp.

How it works: the attachment URL gets its `width`, `height`, `format` and `quality` query parameters removed and the client loads that URL instead of its own proxy copy. Only the query changes, the host stays the one Discord gave for the attachment.

Settings: `originalImagesInChat` (default off). Off, only images up to 2000x1200 pixels are swapped for the original and larger ones keep the client's optimised copy. On, every image attachment is loaded from the original, which may lag with very large or animated files.

Limits: Vencord loads a resized proxy copy at full resolution in the default mode and freezes animated images while the window is unfocused. A plugin cannot build those proxy URLs (a replacement must stay on the host of the attachment), so the default mode uses the original file for images within the pixel limit. Only attachments are changed, not images inside embeds. At most 10 attachments per message are covered.

Permissions: `message-intercept`, `storage`.

Port of the Vencord plugin fixImagesQuality.
