use std::time::Duration;

use gpui::{App, AppContext as _, ClickEvent, FutureExt as _, Keystroke, Window};
use plugin_host::Hook;
use plugin_host::api::model::{
	ChannelClick, ClickKind, ClickTarget, LinkVerdict, MessageClick, MessageRef, Modifiers, UiContext,
};
use protocol::{Message, Snowflake};
use session::{Command, Reply};
use state::ClientOptions;

use super::bridge::PluginBridge;

const LINK_VERDICT_WAIT: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Default)]
pub struct LinkOrigin {
	pub guild: Option<Snowflake>,
	pub channel: Option<Snowflake>,
	pub message: Option<Snowflake>,
}

pub fn client_options(cx: &App) -> ClientOptions {
	cx.try_global::<PluginBridge>()
		.map(|bridge| *bridge.data().read(cx).state().client_options())
		.unwrap_or_default()
}

fn modifiers_of(held: gpui::Modifiers) -> Modifiers {
	[
		(held.control, Modifiers::CTRL),
		(held.shift, Modifiers::SHIFT),
		(held.alt, Modifiers::ALT),
		(held.platform, Modifiers::META),
	]
	.into_iter()
	.filter(|(active, _)| *active)
	.fold(Modifiers::empty(), |all, (_, flag)| all | flag)
}

fn kind_of(event: &ClickEvent) -> ClickKind {
	if event.is_middle_click() {
		ClickKind::Middle
	} else if event.click_count() >= 2 {
		ClickKind::Double
	} else {
		ClickKind::Single
	}
}

pub fn message_click(
	message: &Message,
	guild: Option<Snowflake>,
	event: &ClickEvent,
	target: ClickTarget,
	cx: &mut App,
) -> bool {
	let at = (message.guild_id.or(guild), message.channel_id, message.id);
	message_click_at(at, event, target, cx)
}

pub fn message_click_at(
	at: (Option<Snowflake>, Snowflake, Snowflake),
	event: &ClickEvent,
	target: ClickTarget,
	cx: &mut App,
) -> bool {
	send_message_click(
		message_ref(at),
		kind_of(event),
		modifiers_of(event.modifiers()),
		target,
		cx,
	)
}

pub fn message_click_here(
	at: (Option<Snowflake>, Snowflake, Snowflake),
	target: ClickTarget,
	window: &Window,
	cx: &mut App,
) -> bool {
	send_message_click(
		message_ref(at),
		ClickKind::Single,
		modifiers_of(window.modifiers()),
		target,
		cx,
	)
}

fn message_ref((guild, channel, message): (Option<Snowflake>, Snowflake, Snowflake)) -> MessageRef {
	MessageRef {
		id: message.get(),
		channel_id: channel.get(),
		guild_id: guild.map(Snowflake::get),
	}
}

pub fn watching_messages(cx: &App) -> bool {
	cx.try_global::<PluginBridge>().is_some_and(|bridge| {
		bridge
			.data()
			.read(cx)
			.session()
			.plugin_watches()
			.watches_messages()
	})
}

fn send_message_click(
	message: MessageRef,
	kind: ClickKind,
	modifiers: Modifiers,
	target: ClickTarget,
	cx: &mut App,
) -> bool {
	let Some(bridge) = cx.try_global::<PluginBridge>() else {
		return false;
	};
	let session = bridge.data().read(cx).session();
	if !session.plugin_watches().watches_messages() {
		return false;
	}
	let click = MessageClick {
		message,
		kind,
		modifiers,
		target,
	};
	if !session.plugins().click_watchers(&click) {
		return false;
	}
	bridge.send(Command::PluginMessageClick(click), cx);
	true
}

pub fn channel_click(guild: Option<Snowflake>, channel: Snowflake, event: &ClickEvent, cx: &mut App) -> bool {
	let Some(bridge) = cx.try_global::<PluginBridge>() else {
		return false;
	};
	let session = bridge.data().read(cx).session();
	if !session.plugin_watches().watches_channels() {
		return false;
	}
	let click = ChannelClick {
		guild_id: guild.map(Snowflake::get),
		channel_id: channel.get(),
		kind: kind_of(event),
		modifiers: modifiers_of(event.modifiers()),
	};
	if !session.plugins().channel_click_watchers(&click) {
		return false;
	}
	bridge.send(Command::PluginChannelClick(click), cx);
	true
}

fn chord_of(keystroke: &Keystroke) -> String {
	let held = keystroke.modifiers;
	let mut chord = String::with_capacity(24);
	for (active, name) in [
		(held.control, "ctrl+"),
		(held.alt, "alt+"),
		(held.shift, "shift+"),
		(held.platform, "meta+"),
	] {
		if active {
			chord.push_str(name);
		}
	}
	chord.push_str(&keystroke.key);
	chord
}

pub fn keybind(keystroke: &Keystroke, cx: &mut App) -> bool {
	let Some(bridge) = cx.try_global::<PluginBridge>() else {
		return false;
	};
	let keybinds = bridge.data().read(cx).session().plugin_keybinds();
	if keybinds.is_empty() {
		return false;
	}
	let chord = chord_of(keystroke);
	if keybinds.lookup(&chord).is_none() {
		return false;
	}
	bridge.send(Command::PluginKeybind { chord }, cx);
	true
}

pub fn autocomplete_declared(cx: &App) -> bool {
	cx.try_global::<PluginBridge>().is_some_and(|bridge| {
		bridge
			.data()
			.read(cx)
			.session()
			.plugins()
			.declares(Hook::AutocompleteEmoji)
	})
}

pub fn link_hook_declared(cx: &App) -> bool {
	cx.try_global::<PluginBridge>().is_some_and(|bridge| {
		bridge
			.data()
			.read(cx)
			.session()
			.plugins()
			.declares(Hook::LinkClick)
	})
}

pub fn defer_link(
	url: &str,
	origin: LinkOrigin,
	resolved: impl FnOnce(LinkVerdict, &mut Window, &mut App) + 'static,
	window: &mut Window,
	cx: &mut App,
) {
	let (reply, verdict) = Reply::channel();
	cx.global::<PluginBridge>().send(
		Command::PluginLinkClick {
			url: url.to_owned(),
			context: UiContext {
				guild_id: origin.guild.map(Snowflake::get),
				channel_id: origin.channel.map(Snowflake::get),
				message_id: origin.message.map(Snowflake::get),
				user_id: None,
				url: Some(url.to_owned()),
			},
			reply,
		},
		cx,
	);
	let handle = window.window_handle();
	let executor = cx.background_executor().clone();
	cx.spawn(async move |cx| {
		let verdict = verdict
			.with_timeout(LINK_VERDICT_WAIT, &executor)
			.await
			.ok()
			.and_then(Result::ok)
			.unwrap_or(LinkVerdict::Pass);
		cx.update_window(handle, move |_, window, cx| resolved(verdict, window, cx))
			.ok();
	})
	.detach();
}
