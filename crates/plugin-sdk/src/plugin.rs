use crate::md::{self, Md};
use crate::types::{DisplayOverride, Event, MessageView, OutgoingMessage, SendDecision, User};
use crate::ui::{self, Ui};

pub trait Plugin: Default + 'static {
	fn init(&mut self) -> Result<(), String> {
		Ok(())
	}

	fn on_event(&mut self, _event: Event) {}

	fn render_message(&mut self, _msg: &MessageView, _ast: Vec<Md>) -> Option<Vec<Md>> {
		None
	}

	fn before_send(&mut self, _msg: OutgoingMessage) -> SendDecision {
		SendDecision::Pass
	}

	fn filter_message(&mut self, _msg: &MessageView) -> bool {
		true
	}

	fn display_user(&mut self, _guild_id: Option<u64>, _user: &User) -> Option<DisplayOverride> {
		None
	}

	fn on_ui_action(&mut self, _id: &str, _payload: &str) -> Vec<Ui> {
		Vec::new()
	}

	fn on_command(&mut self, _name: &str, _args: &str, _channel_id: u64) -> Result<(), String> {
		Ok(())
	}

	fn settings_schema(&self) -> String {
		"{}".to_owned()
	}
}

#[doc(hidden)]
pub mod __private {
	use super::*;
	use crate::types::{MdTree, UiTree};

	pub fn render_message<P: Plugin>(plugin: &mut P, msg: MessageView, ast: MdTree) -> Option<MdTree> {
		plugin
			.render_message(&msg, md::from_tree(&ast))
			.map(|nodes| md::to_tree(&nodes))
	}

	pub fn on_ui_action<P: Plugin>(plugin: &mut P, id: String, payload: String) -> UiTree {
		ui::to_tree(&plugin.on_ui_action(&id, &payload))
	}
}

#[doc(hidden)]
#[macro_export]
macro_rules! __export_guest {
	($plugin:ty) => {
		impl $crate::bindings::exports::oxidcord::plugin::guest::Guest for OxidcordExport {
			fn init() -> ::core::result::Result<(), ::std::string::String> {
				STATE.with(|state| $crate::Plugin::init(&mut *state.borrow_mut()))
			}

			fn on_event(event: $crate::types::Event) {
				STATE.with(|state| $crate::Plugin::on_event(&mut *state.borrow_mut(), event))
			}

			fn render_message(
				msg: $crate::types::MessageView,
				ast: $crate::types::MdTree,
			) -> ::core::option::Option<$crate::types::MdTree> {
				STATE.with(|state| $crate::__private::render_message(&mut *state.borrow_mut(), msg, ast))
			}

			fn before_send(msg: $crate::types::OutgoingMessage) -> $crate::types::SendDecision {
				STATE.with(|state| $crate::Plugin::before_send(&mut *state.borrow_mut(), msg))
			}

			fn filter_message(msg: $crate::types::MessageView) -> bool {
				STATE.with(|state| $crate::Plugin::filter_message(&mut *state.borrow_mut(), &msg))
			}

			fn display_user(
				guild_id: ::core::option::Option<u64>,
				user: $crate::types::User,
			) -> ::core::option::Option<$crate::types::DisplayOverride> {
				STATE.with(|state| $crate::Plugin::display_user(&mut *state.borrow_mut(), guild_id, &user))
			}

			fn on_ui_action(
				id: ::std::string::String,
				payload: ::std::string::String,
			) -> $crate::types::UiTree {
				STATE.with(|state| $crate::__private::on_ui_action(&mut *state.borrow_mut(), id, payload))
			}

			fn on_command(
				name: ::std::string::String,
				args: ::std::string::String,
				channel_id: u64,
			) -> ::core::result::Result<(), ::std::string::String> {
				STATE.with(|state| {
					$crate::Plugin::on_command(&mut *state.borrow_mut(), &name, &args, channel_id)
				})
			}

			fn settings_schema() -> ::std::string::String {
				STATE.with(|state| $crate::Plugin::settings_schema(&*state.borrow()))
			}
		}
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __export_state {
	($plugin:ty) => {
		struct OxidcordExport;

		::std::thread_local! {
			static STATE: ::std::cell::RefCell<$plugin> =
				::std::cell::RefCell::new(<$plugin as ::core::default::Default>::default());
		}
	};
}

#[cfg(not(feature = "ext"))]
#[macro_export]
macro_rules! export_plugin {
	($plugin:ty) => {
		const _: () = {
			$crate::__export_state!($plugin);
			$crate::__export_guest!($plugin);
	$crate::bindings::export_guest!(OxidcordExport with_types_in $crate::bindings);
		};
	};
	($plugin:ty, ext) => {
		::core::compile_error!("export_plugin!(T, ext) needs the `ext` feature of oxidcord-plugin-sdk");
	};
}

#[cfg(feature = "ext")]
#[macro_export]
macro_rules! export_plugin {
	($plugin:ty) => {
		const _: () = {
			$crate::__export_state!($plugin);
			$crate::__export_guest!($plugin);

			impl $crate::bindings::exports::oxidcord::plugin::hooks::Guest for OxidcordExport {
				fn render_view(
					_point: ::std::string::String,
					_context: $crate::model::UiContext,
				) -> ::core::option::Option<$crate::model::ViewTree> {
					::core::option::Option::None
				}

				fn on_view_event(_event: $crate::model::ViewEvent) -> $crate::model::ViewReply {
					$crate::__private_ext::quiet()
				}

				fn on_menu_item(
					_menu: ::std::string::String,
					_key: ::std::string::String,
					_context: $crate::model::UiContext,
				) {
				}

				fn intercept_message(
					_msg: $crate::model::MessageFull,
					_content: $crate::model::RichTree,
				) -> $crate::model::MessagePatch {
					$crate::__private_ext::keep()
				}

				fn on_timer(_id: u64, _tag: ::std::string::String) {}

				fn on_emoji_pick(
					_emoji: $crate::model::EmojiRef,
					_channel_id: u64,
				) -> $crate::model::PickVerdict {
					$crate::model::PickVerdict::Default
				}
			}

	$crate::bindings::export_guest!(OxidcordExport with_types_in $crate::bindings);
		};
	};
	($plugin:ty, ext) => {
		const _: () = {
			$crate::__export_state!($plugin);
			$crate::__export_guest!($plugin);

			impl $crate::bindings::exports::oxidcord::plugin::hooks::Guest for OxidcordExport {
				fn render_view(
					point: ::std::string::String,
					context: $crate::model::UiContext,
				) -> ::core::option::Option<$crate::model::ViewTree> {
					STATE.with(|state| {
						$crate::__private_ext::render_view(&mut *state.borrow_mut(), point, context)
					})
				}

				fn on_view_event(event: $crate::model::ViewEvent) -> $crate::model::ViewReply {
					STATE.with(|state| $crate::__private_ext::on_view_event(&mut *state.borrow_mut(), event))
				}

				fn on_menu_item(
					menu: ::std::string::String,
					key: ::std::string::String,
					context: $crate::model::UiContext,
				) {
					STATE.with(|state| {
						$crate::PluginExt::on_menu_item(&mut *state.borrow_mut(), &menu, &key, &context)
					})
				}

				fn intercept_message(
					msg: $crate::model::MessageFull,
					content: $crate::model::RichTree,
				) -> $crate::model::MessagePatch {
					STATE.with(|state| {
						$crate::__private_ext::intercept_message(&mut *state.borrow_mut(), msg, content)
					})
				}

				fn on_timer(id: u64, tag: ::std::string::String) {
					STATE.with(|state| $crate::PluginExt::on_timer(&mut *state.borrow_mut(), id, &tag))
				}

				fn on_emoji_pick(
					emoji: $crate::model::EmojiRef,
					channel_id: u64,
				) -> $crate::model::PickVerdict {
					STATE.with(|state| {
						$crate::PluginExt::on_emoji_pick(&mut *state.borrow_mut(), &emoji, channel_id)
					})
				}
			}

	$crate::bindings::export_guest!(OxidcordExport with_types_in $crate::bindings);
		};
	};
}
