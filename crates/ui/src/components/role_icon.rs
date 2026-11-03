use gpui::{AnyElement, App, Rems};
use state::RoleIcon;

use super::emoji::{remote_icon, unicode_emoji};
use crate::theme::rem_to_pixels;

pub fn role_icon(icon: &RoleIcon, size: Rems, cx: &App) -> AnyElement {
	match icon {
		RoleIcon::Emoji(glyph) => unicode_emoji(glyph, size, cx),
		RoleIcon::Image { role_id, hash } => {
			let pixels = (rem_to_pixels(size) * 2.) as u32;
			remote_icon(
				&media::cdn::role_icon_url(role_id.get(), hash, pixels),
				size,
				pixels,
				cx,
			)
		}
	}
}
