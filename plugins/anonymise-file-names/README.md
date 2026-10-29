# Anonymise File Names

Renames the files you upload right before the message is sent, keeping the extension (`.tar.gz` style double extensions included). A mask button in the composer toolbar switches anonymous names on or off for the next send, and goes back to `anonymiseByDefault` after a message with files went out.

Settings: `anonymiseByDefault` (default on), `method` (`random`, `consistent` or `timestamp`, default `random`), `randomisedLength` (default 7) and `consistent` (default `image`).

Limitations: the toggle is one switch for the whole composer, not one per file. The toolbar button shows the state with its color, Vencord shows it in the attachment bar.

Permissions: `message-send`, `ui-mount`, `storage`.

Port of the Vencord plugin anonymiseFileNames.
