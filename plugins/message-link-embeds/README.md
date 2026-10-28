# Message Link Embeds

Adds a preview under messages that link another message (`discord.com/channels/<server>/<channel>/<message>`). Each preview is an embed card with the author as title, the text of the message, the first image attachment, a footer with `Server - #channel`, `Direct Message` or `Group DM`, and a Jump button that opens the channel and scrolls to the message.

If the client does not hold the linked message yet, the plugin fetches it and redraws the message. This runs for every message with a link you see. Fetches are paced: at most two per step, the host limits them further and a rate limited step waits two seconds. A message that cannot be fetched (deleted, no access) is not asked for again until the plugin restarts. Up to 5 links per message are shown and the last 128 linked messages are kept.

Settings:
- `messageBackgroundColor` (default off) gives the card a darker stripe.
- `automodEmbeds` (`never` by default): `always` writes the preview as one compact line (`**author** text`) without a title, `prefer` does that unless the linked message has files other than images or embeds.
- `listMode` (`blacklist` by default) and `idList`: server, channel or author ids, separated by commas. In blacklist mode a message is skipped when one of its ids is listed, in whitelist mode it is only shown when one is listed.

Limits: the preview is an embed card, not a real message with avatar, reactions and embeds, so attachments other than the first image are not drawn. Links to direct messages are skipped because the plugin has no `context-private`. Only channels the client knows are previewed. The Vencord button that clears the linked message cache is not part of this port, restart the plugin instead.

Permissions: `message-intercept`, `context-timeline`, `context-directory`, `actions-navigate`, `actions-compose`, `storage`.

Port of the Vencord plugin messageLinkEmbeds.
