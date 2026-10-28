# Translate

Translates messages and your own draft.

- A language button on the message hover bar and `Translate` in the message menu translate that message. The translation shows under the message with the detected language and a `Dismiss` link, and stays until you dismiss it or restart the app (the last 100 are kept).
- A button in the composer toolbar translates what you typed and puts the translation back into the composer.

The text is sent to Google Translate (`translate.googleapis.com`, no key) or to DeepL Free (`api-free.deepl.com`, needs an API key). Nothing is sent until you click.

Settings: `service` (`google` or `deepl`), `deeplApiKey`, `receivedInput` and `receivedOutput` (languages for messages you read, defaults `auto` and `en`), `sentInput` and `sentOutput` (languages for your draft, defaults `auto` and `en`). Languages are codes such as `de`, `fr` or `pt-br`. DeepL needs a code from its own list and does not take `auto` as a target.

Limits: the Vencord plugin also offers DeepL Pro, Kagi and automatic translation of every message you send, those are not part of this port. There is no language picker dialog, use the settings. The translation is drawn as a block under the message because annotation lines are limited to 200 bytes. Messages in direct messages cannot be translated (no `context-private`). At most 4000 characters are sent per request.

Permissions: `ui-mount`, `net-fetch`, `message-intercept`, `context-timeline`, `composer`, `storage`. Hosts: `translate.googleapis.com`, `api-free.deepl.com`.

Port of the Vencord plugin translate.
