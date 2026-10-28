# Review DB

Adds a collapsed `Reviews (N)` section to the profile popout of a user. It lists the ten newest reviews from the public ReviewDB service with the author, the time and the text. The reviews load the first time you open a popout and are kept for ten minutes (one minute after a failure), `Refresh` loads them again.

Opening a popout sends that user's id to `manti.vendicated.dev`, the ReviewDB API host, which is why the plugin asks for network access to that one host.

Settings: `showWarning` (the respect reminder at the top, default on) and `hideTimestamps` (default off).

This is a partial port, read only. There is no login with ReviewDB, so you cannot write, delete, report or vote on reviews, there is no list of blocked users and no notification about new reviews, and server reviews are not shown. The Vencord settings `notifyReviews` and `hideBlockedUsers` are left out for that reason. Use the ReviewDB website to review someone.

Permissions: `ui-mount`, `net-fetch` (host `manti.vendicated.dev`), `storage`.

Port of the Vencord plugin reviewDB.
