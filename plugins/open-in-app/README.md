# Open In App

When you click a link to a music or game service, opens it in that service's app instead of the browser.

| Setting | Links | App address |
| --- | --- | --- |
| `spotify` | open.spotify.com tracks, albums, artists, playlists, users, episodes | `spotify:track:<id>` |
| `steam` | store.steampowered.com, help.steampowered.com, steamcommunity.com | `steam://openurl/<link>` |
| `tidal` | tidal.com and listen.tidal.com tracks, albums, artists, playlists, users, videos, mixes | `tidal://track/<id>` |
| `itunes` | music.apple.com albums, artists, playlists, songs, curators | `music://music.apple.com/us/...` |

All four settings are on by default. Links that match nothing, or a service you switched off, open as usual.

Limits: the manifest asks for the schemes spotify, steam, tidal and music and you approve each one. The `epic` setting is dropped because no Epic Games scheme is allowed. Short links (spotify.link, s.team) are not followed because that needs a network request. Apple Music opens through the `music` scheme instead of `itunes`. Opening an app address counts against the `open-url` limit of one per 10 seconds.

Permissions: `actions-open-url`, `storage`.

Port of the Vencord plugin openInApp.
