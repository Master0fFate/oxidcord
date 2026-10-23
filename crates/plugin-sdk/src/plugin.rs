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

#[macro_export]
macro_rules! export_plugin {
	($plugin:ty) => {
		const _: () = {
			struct OxidcordExport;

			::std::thread_local! {
				static STATE: ::std::cell::RefCell<$plugin> =
					::std::cell::RefCell::new(<$plugin as ::core::default::Default>::default());
			}

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
					STATE
						.with(|state| $crate::Plugin::display_user(&mut *state.borrow_mut(), guild_id, &user))
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

	$crate::bindings::export_guest!(OxidcordExport with_types_in $crate::bindings);
		};
	};
}
