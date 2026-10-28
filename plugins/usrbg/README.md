# USRBG

Shows the banner a user registered at USRBG in their profile popout, so anyone can have a banner without Nitro.

The plugin downloads the public USRBG index from usrbg.is-hardly.online (about 6 MB, 210 thousand users), keeps a compact copy in its storage and refreshes it once a day, one second after start when there is no copy yet. Users already on screen get their banner as soon as the first copy is ready, and after a restart the saved copy is used at once.

Settings:
- `nitroFirst`: when a user has both a Nitro banner and a USRBG banner use the Nitro one. Default on. The check only knows about profiles the client already loaded, a user whose profile is not loaded gets the USRBG banner.

Limits: the voice background setting of Vencord (`voiceBackground`) is dropped, the client has no voice tile backgrounds. The download needs the host approval for usrbg.is-hardly.online, both for the index and for the banner images. The index is bigger than the old 4 MB fetch limit, the host limit is 8 MB now.

Permissions: `user-display`, `net-fetch`, `context-profiles`, `storage`.

Port of the Vencord plugin usrbg.
