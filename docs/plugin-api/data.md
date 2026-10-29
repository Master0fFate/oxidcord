# Reading client data

Extended plugins read the state of the client through the `client` interface (API 1.2). In the SDK these are the functions of `oxidcord_plugin_sdk::data`. Every read is gated by a permission, returns `result<_, host-error>`, and works on the data the client already holds unless the table says it can load.

## Rules that apply to every read

- **Permission.** A read without its permission fails with `host-error::invalid("permission <name> is not granted")`. See the table in `overview.md`.
- **Direct messages.** Data from a DM or group DM needs `context-private`. Without it the read answers as if the channel did not exist (`none`, an empty list, `false`) or `unavailable`. A channel the client does not know counts as a guild channel and is refused, so an unknown id never leaks DM data.
- **Cache and load.** Reads marked "loads" answer from the client state or its short cache first. On a miss they ask the session to fetch the data over REST, but only from the Event, Timer and user action contexts (see "Call contexts" in `overview.md`). In `init`, render, intercept, display, decorate, autocomplete and notification hooks a miss returns `unavailable`. A load blocks the plugin thread for up to 5 seconds and does not count against the hook deadline. Loads for the same key from several plugins share one request.
- **Rate limits.** Token buckets per plugin, shared by the functions listed in the table. A call over the limit returns `rate-limited`.
- **Size.** Plugin-visible lists are capped as stated per function.

## Functions

| Function | Permission | Returns | Notes |
| --- | --- | --- | --- |
| `list-relationships() -> list<relationship>` | `context-relationships` | Friends, blocked users, pending requests and implicit relationships | Implicit means a non-bot user who shares two or more loaded servers with you, at most 1000, recomputed at most every 30 s. `nickname` is the friend nickname |
| `user-presence(user-id) -> option<presence>` | `context-presence` | The presence the client holds for a user, or `none` | The client only has presences for friends and for members of servers it loaded |
| `idle-seconds() -> u32` | `context-presence` | Seconds since the last keyboard or mouse input of the system | `unavailable` on Linux |
| `read-state(channel-id) -> option<read-state>` | `context-read-state` | Read state of one channel | `none` for unknown channels and for DMs without `context-private` |
| `unread-channels(guild-id: option) -> list<read-state>` | `context-read-state` | Channels that are unread or have mentions, in one server or in all | At most 2000. DMs are included only with `context-private` |
| `channel-muted(channel-id) -> bool` | `context-read-state` | Whether the channel or its server mute applies | `false` for DMs without `context-private` |
| `list-roles(guild-id) -> list<role-info>` | `context-roles` | Roles of a server | `unavailable` when the server is unknown |
| `member-info(guild-id, user-id) -> option<member-info>` | `context-roles` | A loaded member | `none` when the member is not in the loaded member list |
| `channel-permissions(channel-id, user-id: option) -> u64` | `context-roles` | Permission bits the user (default: you) has in the channel, overwrites applied | DMs return the fixed DM permission set and need `context-private`; an unknown channel is `unavailable` |
| `guild-detail(guild-id) -> option<guild-detail>` | `context-directory` | Server details | `presence-count` is always `none` |
| `channel-detail(channel-id) -> option<channel-detail>` | `context-directory` | Channel details | `none` for DMs without `context-private`. For DMs `name` is the recipient list name |
| `list-folders() -> list<folder>` | `context-directory` | The server rail in order | A server outside a folder is a `folder` with no `id` and one guild |
| `user-profile(user-id) -> option<user-profile>` | `context-profiles` | Profile of a user | Loads. Cached profiles are returned in any context. Loads share the `lookup` limit with `fetch-user`: burst 5, then 1 per 2 s |
| `list-sessions() -> list<auth-session>` | `context-sessions` | Logged in sessions of the account | The first call returns `unavailable` and queues the load; later calls return the list. A failed load is retried after 60 s |
| `client-info() -> client-info` | `context-basic` | Client version, platform, uptime, startup marks, locale | |
| `voice-detail() -> option<voice-detail>` | `context-voice` | The call you are in | `none` for DM calls without `context-private` |
| `fetch-message(channel-id, message-id) -> option<message-full>` | `context-timeline` | A message by id | Loads (`messages?around=<id>&limit=1`). Looks in the channel cache and the last 128 fetched messages first. Burst 5, then 1 per 2 s. `none` for DMs without `context-private` |
| `message-meta(message) -> option<message-meta>` | `context-timeline` | Render-time facts about a cached message | Cache only. `none` for DMs without `context-private` |
| `reaction-users(message, emoji, limit) -> list<user>` | `context-timeline` | Users who reacted with an emoji | Loads. `limit` is clamped to 1 to 100. Burst 5, then 1 per 2 s. Empty for DMs without `context-private` |
| `fetch-user(user-id) -> option<user>` | `context-members` | A user by id | Loads. Cached users are returned in any context. Shares the `lookup` limit with `user-profile` |
| `attachment-bytes(message, attachment-id) -> list<u8>` | `context-attachments` | The attachment file | Loads in Event, Timer and user action contexts only. The message must be in the cache or fetched. Up to 8 MiB, over that `invalid("response is too large")`. `unavailable` for DMs without `context-private`. 10 per minute, shared with `avatar-bytes` |
| `avatar-bytes(user-id, size) -> list<u8>` | `context-attachments` | The avatar image of a user | Same rules as `attachment-bytes`. `size` is clamped to 16 to 512 |
| `list-favorite-emojis() -> list<emoji-ref>` | `context-expressions` | Favourite emoji | Always empty at the moment |
| `list-plugins() -> list<plugin-summary>` | `plugins-list` | Installed plugins | `ext` is true when the manifest declares `hooks` |
| `composer-get-draft() -> string` | `composer` | Draft text of the selected channel | Empty in a DM without `context-private` |

The 1.1 reads `list-emojis`, `list-stickers`, `get-message` and `channel-messages-full` are described in `overview.md`.

## Records

Times are Unix milliseconds. Ids are snowflakes (`u64`). Colors are `0xRRGGBB` integers.

### `relationship`

| Field | Type | Meaning |
| --- | --- | --- |
| `user` | `user` | The other user |
| `kind` | `relationship-kind` | `friend`, `blocked`, `pending-incoming`, `pending-outgoing`, `implicit` |
| `since-ms` | `option<s64>` | When the relationship started |
| `nickname` | `option<string>` | Friend nickname |

### `presence` and `activity-view`

| Field | Type | Meaning |
| --- | --- | --- |
| `user-id` | `snowflake` | |
| `status` | `string` | `online`, `idle`, `dnd`, `invisible`, `offline` |
| `platforms` | `list<string>` | Clients that are online: `desktop`, `mobile`, `web` |
| `activities` | `list<activity-view>` | `kind` (activity type), `name`, `details`, `state`, `emoji`, `start-ms`, `end-ms` |

### `read-state`

| Field | Type | Meaning |
| --- | --- | --- |
| `channel-id` | `snowflake` | |
| `last-message-id` | `option<snowflake>` | Newest message the client knows |
| `last-read-id` | `option<snowflake>` | Last message you read |
| `mention-count` | `u32` | |
| `unread` | `bool` | |
| `muted` | `bool` | |

### `role-info`

`id`, `name`, `color` (primary role color, 0 for none), `position` (`s32`), `hoist`, `icon` (hash or `none`), `permissions` (`u64` bits), `mentionable`.

### `member-info`

| Field | Type | Meaning |
| --- | --- | --- |
| `user` | `user` | |
| `nick` | `option<string>` | Server nickname |
| `roles` | `list<snowflake>` | |
| `top-role` | `option<snowflake>` | Role with the highest position |
| `color` | `option<u32>` | Name color from the highest colored role |
| `joined-ms` | `option<s64>` | |
| `timeout-until-ms` | `option<s64>` | Only while the timeout is in the future |
| `premium-since-ms` | `option<s64>` | Boosting since |
| `pending` | `bool` | Has not passed membership screening |

### `guild-detail`

`id`, `name`, `icon`, `banner`, `description`, `owner-id`, `member-count`, `presence-count` (always `none`), `boost-tier` (`u8`), `boost-count`, `created-ms` (from the id), `features` (feature strings), `vanity`.

### `channel-detail`

`id`, `guild-id`, `name`, `kind` (channel type number), `topic`, `nsfw`, `parent-id`, `position`, `recipients` (user ids, DMs), `owner-id`, `last-message-id`, `rate-limit` (seconds of slow mode), `created-ms`.

### `folder`

`id` (`none` for a server outside a folder), `name`, `color`, `guilds` (server ids in order).

### `user-profile` and `connection`

| Field | Type | Meaning |
| --- | --- | --- |
| `user` | `user` | |
| `bio` | `option<string>` | |
| `pronouns` | `option<string>` | Global pronouns |
| `banner` | `option<string>` | Banner hash |
| `accent-color` | `option<u32>` | |
| `connections` | `list<connection>` | `kind`, `name`, `id` (empty), `verified`, `url` (`none`) |
| `premium-since-ms` | `option<s64>` | |
| `guild-nick` | `option<string>` | Server nickname from the profile response |
| `mutual-guilds` | `list<snowflake>` | |

### `auth-session`

`id` (hash, not the token), `client-os`, `client-platform`, `location`, `approx-last-used-ms`, `current` (true when the id hash matches this session; false for all entries when none matches).

### `client-info`

`version`, `platform` (`windows`, `macos`, `linux`), `uptime-ms`, `startup` (list of phase name and milliseconds since launch: `launch`, `gateway-connected`, `ready`), `locale`.

### `voice-detail`

`base` (a `voice-info`: guild id, channel id, self mute and deaf, participants, speaking, e2ee), `started-ms` (when the voice session connected), `channel-name`, `guild-name`, `self-video` (camera or screen share).

### `message-meta`

| Field | Type | Meaning |
| --- | --- | --- |
| `guild-id` | `option<snowflake>` | |
| `reply-to` | `option<message-ref>` | The replied message, `none` for forwards |
| `flags` | `u64` | Discord message flags |
| `latency-ms` | `option<u32>` | Time between sending and the server acknowledgement, only for messages this client sent |
| `author-color` | `option<u32>` | Role color of the author in a server |
| `pinned` | `bool` | |
| `edited-ms` | `option<s64>` | |
| `nsfw-channel` | `bool` | |

### `plugin-summary`

`id`, `name`, `version`, `enabled`, `ext`.

## Rate limit summary

| Bucket | Functions | Limit |
| --- | --- | --- |
| `lookup` | `user-profile`, `fetch-user` | burst 5, 1 per 2 s |
| `fetch-message` | `fetch-message` | burst 5, 1 per 2 s |
| `reaction-users` | `reaction-users` | burst 5, 1 per 2 s |
| `attachment-bytes` | `attachment-bytes`, `avatar-bytes` | burst 10, 1 per 6 s |
| none | the other reads | no limit, they only read memory |
