# Blur NSFW

Blurs the attachments of messages in age restricted channels until you hover them.

Settings: `blurAmount` (default 10) is accepted so the setting list matches the Vencord plugin, but it does nothing. The client blurs with a tiny upscaled copy of the attachment and a fixed "Hover to reveal" label.

Limits: only attachments are blurred, images inside embeds and stickers are not. At most 10 attachments per message are covered.

Permissions: `message-intercept`.

Port of the Vencord plugin blurNsfw.
