# Read All Notifications

Adds a button above the server list that marks every server with unread channels as read. A short message says how many servers were marked.

Limits: marking a server as read is limited to five calls per click and then one per second, so with more than five unread servers the message says to click again for the rest. Direct messages are not touched, only servers. Muted channels count as unread when the client reports them as unread.

Permissions: `ui-mount`, `context-directory`, `context-read-state`, `actions-ack`.

Port of the Vencord plugin readAllNotificationsButton.
