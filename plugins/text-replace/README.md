# Text Replace

Replaces text in your messages with rules before they are sent. The composer shows its usual indicator that a plugin changed the message. Rules are edited on the settings page of the plugin, below the settings form. A rule tester at the top shows what a message would turn into.

Every rule has a `name`, a `find` text, a `replace` text and an optional `onlyIfIncludes`: when it is set the rule only runs if the message contains that text. Write `\n` in a replacement for a line break.

Simple rules (`stringRules`) replace every occurrence of the find text. Regex rules (`regexRules`) take a pattern, either bare or as `/pattern/flags`, with the flags `g` (default when none are given), `i`, `m` and `s`. Replacements understand `$1`, `$<name>`, `$&` and `$$`. Simple rules run first, then regex rules, each on the output of the previous one, and the final message is trimmed. A message that would end up empty or longer than 4000 bytes is sent unchanged. Invalid regexes are shown in red and skipped.

Limits: the regex engine is `regex-lite`, so look-around and backreferences in the pattern are not supported and the flags `u`, `y` and `v` are ignored. Only one digit groups work in replacements (`$1` to `$9`). Rules cannot be reordered by dragging, delete and add them in the order you want. Vencord skips the plugin in its own rules sharing channel, this port has no such exception.

Permissions: `message-send`, `storage`, `ui-mount`.

Port of the Vencord plugin textReplace.
