# Petpet

Adds the `/petpet <user> [delay]` command. `user` is a mention or a user id, `delay` is the time between frames in milliseconds (default 20, minimum 20, rounded down to 10 ms). The plugin downloads the 128 px avatar of the user, squashes it in 10 frames under a drawn hand and sends the result as `petpet.gif` to the current channel.

Limitations: partial. Vencord also takes an image attachment, an image url, a resolution and a no-server-avatar flag; here only a user is supported, the size is fixed at 128 px, and the avatar is the global one. The hand is drawn by the plugin from simple shapes, it does not look like the hand of the original. The whole job has to fit the 2 second deadline of a command, which is why the size and the frame count are fixed. The avatar download is limited to 10 per minute.

Permissions: `ui-slots`, `context-attachments`, `actions-upload`.

Port of the Vencord plugin petpet.
