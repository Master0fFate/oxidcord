# Direct File Download

Saves attachments straight to disk.

- A download icon button next to every attachment.
- Save as in the attachment menu.

Both open the save dialog of the client and write the file where you choose. Without this plugin the download button the client puts on files only opens the file in the browser.

Limits: no settings. The attachment must be in the loaded messages and at most 100 MiB. Direct messages work only when the client already allows the plugin there (this plugin does not ask for `context-private`).

Permissions: `ui-mount`, `actions-save`.

Port of the Vencord plugin directFileDownload.
