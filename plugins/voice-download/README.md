# Voice Download

Adds a download button to voice messages and other audio attachments (content type `audio/*`).

Clicking it opens the save dialog of the client and saves the file there. The built-in download button of the audio player only opens the file in the browser.

Limits: no settings. The button needs the message data, so it does not show in direct messages (that needs `context-private`, which this plugin does not ask for). Files over 100 MiB are refused by the client.

Permissions: `ui-mount`, `context-timeline`, `actions-save`.

Port of the Vencord plugin voiceDownload.
