# Better GIF Alt Text

Gives GIFs an alt text with their tags or file name instead of plain "GIF", like `GIF - funny cat`.

- GIF attachments get the alt text from their file name: digits are dropped, the extension is cut and the words are split on `,`, `-`, `_` and spaces (at most 20 words).
- GIF embeds (Tenor, Giphy, Imgur) get the words from the page link, for example `tenor.com/view/funny-cat-gif-123` gives `GIF - funny cat`.

Limits: embeds cannot carry an alt text, so a GIF embed is replaced by an embed card that has the alt text as its title, links to the original page and shows the GIF thumbnail as a still image. The thumbnail only shows for the hosts the plugin lists in `image_hosts` (Tenor, Giphy and Imgur media hosts, each one needs your approval), otherwise the card has the title only. The animation of the original embed is lost. Attachments keep their normal look and only get the alt text, which the client shows as a caption.

Permissions: `message-intercept`. Image hosts: `media.tenor.com`, `c.tenor.com`, `media1.tenor.com`, `media.giphy.com`, `i.imgur.com`.

Port of the Vencord plugin betterGifAltText.
