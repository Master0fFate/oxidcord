# Keyword Highlighter

Makes your keywords bold in message text and adds a `keyword: ...` line under the message. When a message from someone else matches, it sends one notification per message, only for messages younger than a minute.

Set the `keywords` setting to a list of words. Matching ignores ASCII case. The list is kept in plugin storage.

Permissions: `message-render`, `storage`, `context-basic` (to skip your own messages), `actions-notify`.
