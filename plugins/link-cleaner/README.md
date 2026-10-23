# Link Cleaner

Removes tracking parameters from every http and https link in a message before it is sent. The composer shows its usual indicator that a plugin changed the message.

Removed parameters: `utm_*`, `fbclid`, `gclid`, `igshid`, `ref_src`, `si` on YouTube and Spotify links, `s` on Twitter and X links. Other parameters and the fragment stay.

Permissions: `message-send`.
