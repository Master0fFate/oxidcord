# Dearrow

Replaces the title and thumbnail of YouTube embeds with the community versions from DeArrow, which are less sensationalist.

For each YouTube link in a message the plugin asks `sponsor.ajay.app` (the DeArrow API) for a community title and thumbnail time. The embed then becomes a card with the new title and the thumbnail from `dearrow-thumb.ajay.app`. Answers are kept in the plugin storage for a day (300 videos at most), a failed lookup is retried after 10 minutes. Lookups are paced: at most two per step, the host limits them further and a rate limited step waits six seconds.

A Show original / Use Dearrow button under the message swaps one message between the original embed and the Dearrow card.

Settings: `hideButton` (default off), `replaceElements` (`both` by default, or `title` or `thumbnail`) and `dearrowByDefault` (default on, off shows the original and the button turns Dearrow on).

Privacy: every YouTube video id in a message you see is sent to DeArrow. Nothing else leaves the client.

Limits: the card has the title, description, link and a footer with the provider, the uploader name of the original embed is not kept. With `title` only, the original thumbnail is drawn as a still image from the YouTube image host (`i.ytimg.com`, which needs your approval, like `dearrow-thumb.ajay.app`). The card replaces the video player of the embed. Only `youtube.com` and `youtu.be` links are looked up.

Permissions: `message-intercept`, `net-fetch`, `storage`. Hosts: `sponsor.ajay.app`. Image hosts: `dearrow-thumb.ajay.app`, `i.ytimg.com`.

Port of the Vencord plugin dearrow.
