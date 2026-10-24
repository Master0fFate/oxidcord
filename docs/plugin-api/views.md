# Views, mount points and menus

Extended plugins (`export_plugin!(T, ext)`) describe interface with view trees. The host validates each tree, renders it with native widgets and theme colors, and sends user input back through `hooks.on-view-event`. Plugins never draw pixels, run code in the UI thread or see raw input. Everything here needs the `ui-mount` permission.

## View trees

A `view-tree` is a flat list of `view-node` plus `roots`. Each node has an optional `key`, an optional `style` and a `kind`. Container kinds list their children by index, and a child must come after its parent. In the SDK, `view::Node` builders produce the tree:

```rust
use oxidcord_plugin_sdk::model::{Space, Tone};
use oxidcord_plugin_sdk::view;

let card = view::column()
	.gap(Space::Md)
	.child(view::text("Build passed").bold())
	.child(view::button("rerun", "Run again").icon("rotate-cw").tone(Tone::Brand));
```

Only nodes with a `key` produce events. The key is echoed back as `view-event.node`.

### Node kinds

| Kind | SDK builder | Notes |
| --- | --- | --- |
| `stack` | `view::column()`, `view::row()`, `.wrap()` | Children laid out vertically or horizontally |
| `scroll` | `view::scroll()` | Scrolls vertically. Give it a fixed `height` or let it fill its parent |
| `spacer` | `view::spacer()` | Takes the free space in a stack |
| `divider` | `view::divider()` | |
| `text` | `view::text(value)`, `view::heading(value)` | `size`, `weight`, `tone`, `color`, `mono`, `italic` |
| `markdown` | `view::markdown(source)` | Parsed by the host markdown parser, up to 8000 bytes |
| `image` | `view::image(url)` | `url`, `emoji` or `avatar` source, up to 512 px. See image rules in `overview.md` |
| `icon` | `view::icon(name)` | A Lucide icon name such as `notebook`, `trash`, `plus` |
| `avatar` | `view::avatar(user_id, size)` | Avatar of a user the client knows |
| `emoji` | `view::emoji(emoji_ref, size)` | Custom emoji by id or a unicode emoji by name |
| `button` | `view::button(key, label)` | `.icon()`, `.tone()`, `.variant()`, `.submit()` |
| `icon-button` | `view::icon_button(key, icon, tooltip)` | |
| `link` | `view::link(label, url)` | http or https, opened through the app's link handling |
| `input`, `textarea` | `view::input(key, placeholder)`, `view::textarea(key, placeholder)` | `.value()` |
| `select` | `view::select(key, placeholder).option(value, label).choose(value)` | Single choice |
| `toggle`, `checkbox` | `view::toggle(key, label, on)`, `view::checkbox(key, label, on)` | |
| `slider` | `view::slider(key, min, max, step, value)` | |
| `progress` | `view::progress(fraction)` | 0.0 to 1.0 |
| `badge` | `view::badge(label)` | |
| `tabs` | `view::tabs().tab(label, content)` | Switching tabs happens in the host |
| `section` | `view::section(title).collapsed(bool)` | Collapsible, the host keeps the open state |
| `popover` | `view::popover(trigger).child(content)` | Child 0 is the trigger, the rest is the popover body |
| `form` | `view::form().key(key)` | Collects the values of keyed descendants on submit |
| `embed` | `view::embed(card)` | An embed card like the ones under messages |
| `user-chip`, `channel-chip` | `view::user_chip(id)`, `view::channel_chip(id)` | Resolved by the host |
| `timestamp` | `view::timestamp(unix_ms, style)` | Discord timestamp styles `t T d D f F R` |

### Style

`style` applies to any node. All fields are optional in the SDK: `.width(Length)`, `.height(Length)` (`auto`, `fill` or `px(n)`), `.fill()`, `.padding(Space)`, `.gap(Space)`, `.align(Align)`, `.justify(Justify)`, `.background(Color)`, `.border(Color)`, `.radius(Radius)`, `.tooltip(text)` and `.disabled(bool)`. Spacing uses the scale `none xs sm md lg xl`.

Colors are either theme tokens or RGBA values, so plugins follow the user's theme by default:

```rust
use oxidcord_plugin_sdk::Color;

view::text("warning").color(Color::token("warning"));
view::row().background(Color::rgb(30, 30, 46));
```

Valid tokens: `background-primary`, `background-secondary`, `background-tertiary`, `background-floating`, `modifier-hover`, `modifier-selected`, `modifier-active`, `text-normal`, `text-muted`, `header-primary`, `header-secondary`, `interactive-normal`, `interactive-hover`, `interactive-muted`, `brand`, `text-on-brand`, `link`, `danger`, `success`, `warning`, `mention-background`, `mention-text`, `divider`, `input-background`. There is no raw CSS and no absolute positioning.

### Limits

2000 nodes per tree, 32 levels, 500 children per container, 4096 bytes per text, 128 bytes per key and label. Nodes that break a rule are dropped, not the whole tree.

## Mount points

Static mount points take a tree that stays until the plugin updates or unmounts it:

```rust
use oxidcord_plugin_sdk::mount;

mount::mount(mount::STATUS_BAR, "clock", [view::text("12:00")], mount::Options::new("Clock"))?;
mount::update(mount::STATUS_BAR, "clock", [view::text("12:01")])?;
mount::unmount(mount::STATUS_BAR, "clock")?;
```

| Point | Where | Notes |
| --- | --- | --- |
| `title-bar` | Window title bar | Compact: buttons show icon and label |
| `status-bar` | Under the composer | Compact |
| `composer-toolbar` | Next to the composer buttons | Compact, icon buttons |
| `message-action` | Message hover toolbar | Compact, the event context carries the message |
| `sidebar-panel` | Under the channel list | Collapsible section titled with `options.title` |
| `right-panel` | Right column, toggled from the top bar | |
| `app-nav` | Server rail | An icon from `options.icon` with `options.title` as tooltip. Clicking it opens the `main-view` mount of the same plugin with the same key |
| `main-view` | Timeline area | A full page, shown while its `app-nav` icon is selected |
| `settings-page` | Under the plugin's settings form | |

`options.order` sorts mounts of one point, lower first.

Contextual points depend on what the user is looking at. A plugin declares that it provides content with `mount::provide(point, options)`. When the point is shown the host calls `render-view(point, context)` on a background thread and caches the result per plugin, point and context. The UI never waits for it: the content appears when the call returns. `mount::invalidate(point)` drops the cache so the next showing calls `render-view` again.

| Point | Context fields |
| --- | --- |
| `channel-header` | `guild-id`, `channel-id` |
| `composer-above` | `guild-id`, `channel-id` |
| `user-popout-section` | `guild-id`, `user-id` |
| `member-list-section` | `guild-id` |

```rust
impl PluginExt for Topic {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		(point == mount::CHANNEL_HEADER).then(|| vec![view::badge(format!("#{}", context.channel_id?))])
	}
}
```

## View events

When the user clicks a keyed button, changes a keyed field or submits a form, the host calls `on-view-event` with a `view-event`:

| Field | Meaning |
| --- | --- |
| `point` | Mount point, `modal`, `toast` or `message-accessory` |
| `mount` | Mount key (message id for accessories) |
| `node` | Key of the node, or of the form on submit |
| `kind` | `click`, `change(text)`, `toggle(bool)`, `select(value)`, `tab(index)` or `submit(values)` |
| `context` | Guild, channel, message and user ids that apply |

Typing does not reach the plugin keystroke by keystroke. A field outside a form sends `change` when the user presses Enter or leaves it. Fields inside a form send nothing on their own: a button with `.submit()` (or Enter in a single line input) sends one `submit` with every keyed field of the form as `(key, value)` pairs, and the form's fields reset afterwards.

The reply can replace the tree of the view the event came from (static mounts, contextual views and modals; a message accessory is replaced the next time the message is intercepted), close an open modal, and show a short toast:

```rust
fn on_view_event(&mut self, event: ViewEvent) -> Reply {
	match (&event.kind, event.node.as_str()) {
		(ViewEventKind::Submit(values), "add-form") => {
			self.add(values);
			Reply::none().replace(self.page()).toast("Saved")
		}
		_ => Reply::none(),
	}
}
```

View events, menu items and commands count as user actions (see `overview.md`). After a plugin handles one, the host renders the messages of the open channel again so that changed plugin state shows up in intercepted messages.

## Context menus

`menu::set(menu, items)` replaces the plugin's items in one menu. Menus: `message-context-menu`, `user-context-menu`, `channel-context-menu`, `server-context-menu`, `link-context-menu`. The desktop client does not have a link context menu yet, so items registered there are accepted but not shown.

```rust
use oxidcord_plugin_sdk::menu;

menu::set(
	menu::MESSAGE,
	[
		menu::item("save", "Save to notes").icon("sticky-note").when(menu::When::HAS_CONTENT),
		menu::item("share", "Share"),
		menu::item("share-link", "As link").parent("share"),
	],
)?;
```

An item is shown only when every flag in `when` holds: `in-guild`, `in-dm`, `own-message`, `others-message`, `has-content`, `has-attachments`, `self-user`, `other-user`. An item with a `parent` becomes an entry of that item's submenu, one level deep. Picking an item calls `on-menu-item(menu, key, context)` with the ids of the message, user, channel or server the menu was opened on.

1.0 plugins that put `button` nodes into a slot named like a menu keep working, their buttons are appended to the menu.

## Toasts and modals

```rust
mount::toast([view::text("Copied")], 3000)?;
mount::open_modal("Rename", [view::form().key("rename").child(view::input("name", "New name"))])?;
```

A toast stays 1.5 to 15 seconds (0 means 5 s). Outside a user action a plugin can show one toast every 5 seconds. `open-modal` works only inside a user action. Events from a modal arrive with point `modal`, and a reply with `replace` changes the modal content while `close` closes it.

## 1.0 slots

`register-slot` trees from 1.0 plugins are converted to view trees and drawn by the same renderer. Their events still reach `on-ui-action(id, payload)` with the 1.0 payload rules: the slot payload for buttons, the text for inputs and `true` or `false` for toggles.
