# Sort Friend Requests

Adds a Requests by date tab to the Friends page. It lists your pending incoming and outgoing friend requests in one list, newest first, with an arrow showing the direction. Requests without a known date go last.

Settings:
- `showDates`: show the date each request arrived on its row. Default off.

Limits: this is a separate tab. The core Friends list and its Pending tab keep their own order, a plugin cannot reorder them. Dates are shown as short dates.

Permissions: `ui-mount`, `context-relationships`, `storage`.

Port of the Vencord plugin sortFriendRequests.
