# Clear URLs

Removes tracking elements from the links in a message before it is sent, using the rule set of the ClearURLs project (`raw.githubusercontent.com/ClearURLs/Rules`). The composer shows its usual indicator that a plugin changed the message.

For every link the plugin follows the provider rules that match it: tracking query parameters are dropped (`rules` and `referralMarketing`), raw patterns are cut out of the link (`rawRules`), provider exceptions are respected, and redirect wrappers such as `google.com/url?q=...` are replaced by their target (`redirections`). Providers that only exist to block requests (`completeProvider`) are ignored.

The rules are downloaded three seconds after the plugin starts when the saved copy is missing or older than a day, and checked again every six hours. The saved copy is kept in plugin storage with the time it was fetched. Until a download works a small built-in list of common parameters (`utm_*`, `fbclid`, `gclid`, `msclkid`, `igshid` and similar) is used. A rule that the regex engine cannot read is skipped.

`link-cleaner` is a small separate plugin with a fixed list. This one uses the full ClearURLs data and needs network access to GitHub.

Limits: the regex engine is `regex-lite`, so a rule with look-around or backreferences would be skipped (none of the current rules use them). Links inside code blocks are cleaned too. Messages are only changed when the result is not empty and fits in 4000 bytes.

Permissions: `message-send`, `net-fetch` (host `raw.githubusercontent.com`), `storage`.

Port of the Vencord plugin clearURLs.
