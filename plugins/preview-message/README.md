# Preview Message

Adds an eye button to the composer toolbar. Pressing it renders the text of your draft as a message preview above the composer, with a Close button. The preview is a snapshot of the draft at the moment you pressed the button, press it again to refresh it.

Limitations: the preview is not a message in the timeline, it is a card above the composer rendered from the draft markdown. Attachments are not previewed: the composer does not expose the files in your draft to plugins, so only the text is shown. The card shows up to 8000 bytes of the draft.

Permissions: `ui-mount`, `composer`.

Port of the Vencord plugin previewMessage.
