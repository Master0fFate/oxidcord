# Image Filename

Shows the file name of image attachments as a tooltip when you hover them.

Settings: `showFullUrl` (default off) shows the full attachment URL instead of the file name.

Limits: only attachments get the tooltip. GIFs and images that come from link embeds (Tenor, Giphy, Imgur) are not covered because embeds cannot carry a tooltip, so the Vencord rule that always shows the URL for GIF hosts does not apply. Tooltips are cut at 200 bytes and at most 10 attachments per message are covered.

Permissions: `message-intercept`, `storage`.

Port of the Vencord plugin imageFilename.
