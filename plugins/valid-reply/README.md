# Valid Reply

Fixes replies whose quote shows as unloaded or deleted because the replied message is not in the client's memory.

The plugin notices a reply whose target message is not loaded, fetches that message and redraws the reply. Because the client draws the quote from the reply payload and not from its message cache, a fetched message cannot fill that quote. The plugin shows the fetched message under the reply instead: an arrow, the author, the first 140 characters of the text and a Jump button that scrolls to the message.

Fetching runs for every such reply you see, not only on hover like Vencord. Fetches are paced: at most two per step, the host limits them further and a rate limited step waits two seconds. A message that cannot be fetched (deleted, no access) is not asked for again until the plugin restarts.

Limits: the quote itself is not repaired, replies whose target the client already holds get no extra line, and the line only shows text (no attachments or embeds). Replies in direct messages need `context-private`, which this plugin does not ask for, so they are skipped.

Permissions: `message-intercept`, `context-timeline`, `actions-compose`.

Port of the Vencord plugin validReply.
