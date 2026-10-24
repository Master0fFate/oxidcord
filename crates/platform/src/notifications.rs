use std::path::PathBuf;
use std::thread;

use crate::Result;

#[derive(Debug, Clone)]
pub struct Notification {
	pub title: String,
	pub body: String,
	pub icon_path: Option<PathBuf>,
	pub app_id: String,
}

fn toast(notification: &Notification) -> notify_rust::Notification {
	let mut toast = notify_rust::Notification::new();
	toast
		.summary(&notification.title)
		.body(&notification.body)
		.appname(&notification.app_id);
	#[cfg(windows)]
	toast.app_id(&notification.app_id);
	#[cfg(all(unix, not(target_os = "macos")))]
	toast.action("default", "");
	if let Some(icon_path) = &notification.icon_path {
		toast.image_path(&icon_path.to_string_lossy());
	}
	toast
}

pub fn show(notification: &Notification) -> Result<()> {
	toast(notification).show()?;
	Ok(())
}

pub fn show_with_click(notification: Notification, on_click: impl FnOnce() + Send + 'static) {
	let spawned = thread::Builder::new()
		.name("oxidcord-notification".to_owned())
		.spawn(move || match toast(&notification).show() {
			Ok(handle) => wait_for_click(handle, on_click),
			Err(error) => tracing::warn!(%error, "could not show the notification"),
		});
	if let Err(error) = spawned {
		tracing::warn!(%error, "could not start the notification thread");
	}
}

#[cfg(not(target_os = "macos"))]
fn wait_for_click(handle: notify_rust::NotificationHandle, on_click: impl FnOnce()) {
	let waited = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
		if response.is_default_action() {
			on_click();
		}
	});
	if let Err(error) = waited {
		tracing::debug!(%error, "notification response was lost");
	}
}

#[cfg(target_os = "macos")]
fn wait_for_click(_: notify_rust::NotificationHandle, _: impl FnOnce()) {}

#[cfg(windows)]
pub fn set_app_user_model_id(id: &str) -> Result<()> {
	use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
	use windows::core::HSTRING;

	unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(id)) }
		.map_err(|error| crate::PlatformError::Os(error.to_string()))
}

#[cfg(not(windows))]
pub fn set_app_user_model_id(_id: &str) -> Result<()> {
	Ok(())
}
