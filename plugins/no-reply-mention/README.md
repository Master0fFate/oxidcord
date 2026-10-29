# No Reply Mention

Decides whether a reply pings the author of the message you reply to. The user and role lists name the people the rule is about, and `shouldPingListed` says what it does with them: on, only listed users and roles are pinged; off, everyone except them is pinged. With empty lists and the default, no reply pings.

Settings: `userList` and `roleList` (ids separated by commas or spaces, default empty), `shouldPingListed` (default on) and `inverseShiftReply` (not applicable).

Limitations: the rule is applied when the reply is sent, so the ping toggle on the reply bar is overridden for every reply the rule covers. `inverseShiftReply` is not applicable because plugins cannot see the Shift state of the composer. The role list needs the author's roles and the replied message in the cache; when either is missing the author counts as not listed. Roles are only read in servers.

Permissions: `message-send`, `context-timeline`, `context-roles`, `storage`.

Port of the Vencord plugin noReplyMention.
