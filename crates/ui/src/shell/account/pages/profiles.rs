use std::io::Cursor;
use std::sync::Arc;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ObjectFit, ParentElement as _, Render, RenderImage, SharedString, StatefulInteractiveElement as _,
	Styled as _, StyledImage as _, Subscription, Window, div, img, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use image::{ImageFormat, RgbaImage};
use protocol::Snowflake;
use session::Command;

use super::widgets::heading;
use crate::components::Avatar;
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::images;
use crate::lookup::user_avatar;
use crate::model::role_color;
use crate::shell::account::{avatar_ring, markdown_text};
use crate::theme::{Theme, with_alpha};

const AVATAR_LIMIT: u32 = 512;
const BANNER_LIMIT: (u32, u32) = (960, 540);
const BIO_LIMIT: usize = 190;
const PREVIEW_AVATAR_PIXELS: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
	User,
	Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
	Avatar,
	Banner,
	ServerAvatar,
}

struct Picked {
	png: Vec<u8>,
	preview: Arc<RenderImage>,
}

pub struct ProfilesPage {
	data: Entity<Data>,
	tab: Tab,
	name: Entity<InputState>,
	pronouns: Entity<InputState>,
	bio: Entity<TextareaState>,
	nick: Entity<InputState>,
	server_bio: Entity<TextareaState>,
	avatar: Option<Picked>,
	banner: Option<Picked>,
	server_avatar: Option<Picked>,
	guild: Option<Snowflake>,
	prefilled: bool,
	_subscriptions: Vec<Subscription>,
}

fn prepare(bytes: &[u8], slot: Slot) -> Option<(Vec<u8>, RgbaImage)> {
	let decoded = image::load_from_memory(bytes).ok()?;
	let (limit_w, limit_h) = match slot {
		Slot::Banner => BANNER_LIMIT,
		Slot::Avatar | Slot::ServerAvatar => (AVATAR_LIMIT, AVATAR_LIMIT),
	};
	let squared = match slot {
		Slot::Banner => decoded,
		Slot::Avatar | Slot::ServerAvatar => {
			let side = decoded.width().min(decoded.height());
			let left = (decoded.width() - side) / 2;
			let top = (decoded.height() - side) / 2;
			decoded.crop_imm(left, top, side, side)
		}
	};
	let fitted = if squared.width() > limit_w || squared.height() > limit_h {
		squared.thumbnail(limit_w, limit_h)
	} else {
		squared
	};
	let rgba = fitted.into_rgba8();
	let mut png = Vec::new();
	rgba.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).ok()?;
	Some((png, rgba))
}

impl ProfilesPage {
	pub fn new(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let name_placeholder = cx.t("profiles-display-name");
		let name = cx.new(|cx| InputState::new(window, cx).placeholder(name_placeholder));
		let pronouns_placeholder = cx.t("profiles-pronouns-placeholder");
		let pronouns = cx.new(|cx| InputState::new(window, cx).placeholder(pronouns_placeholder));
		let bio = cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 8));
		let nick_placeholder = cx.t("profiles-nickname");
		let nick = cx.new(|cx| InputState::new(window, cx).placeholder(nick_placeholder));
		let server_bio = cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 8));
		let mut subscriptions = Vec::new();
		for input in [&name, &pronouns, &nick] {
			subscriptions.push(cx.subscribe(input, |_, _, event: &InputEvent, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}));
		}
		for input in [&bio, &server_bio] {
			subscriptions.push(cx.subscribe(input, |_, _, event: &InputEvent, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}));
		}
		subscriptions.push(
			cx.subscribe_in(&data, window, |this, _, event: &DataEvent, window, cx| {
				if matches!(event, DataEvent::Changed(_)) && !this.prefilled {
					this.prefill(window, cx);
				}
			}),
		);
		if let Some(me) = data.read(cx).me() {
			data.read(cx).send(Command::LoadProfile {
				user_id: me,
				guild_id: None,
			});
		}
		let mut page = Self {
			data,
			tab: Tab::User,
			name,
			pronouns,
			bio,
			nick,
			server_bio,
			avatar: None,
			banner: None,
			server_avatar: None,
			guild: None,
			prefilled: false,
			_subscriptions: subscriptions,
		};
		page.prefill(window, cx);
		page
	}

	fn prefill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let (name, bio, pronouns, loaded) = {
			let data = self.data.read(cx);
			let state = data.state();
			let Some(user) = state.current_user() else {
				return;
			};
			let profile = state.profile(user.id);
			(
				user.global_name.clone().unwrap_or_default(),
				profile
					.and_then(|profile| profile.bio.clone())
					.unwrap_or_default(),
				profile
					.and_then(|profile| profile.pronouns.clone())
					.unwrap_or_default(),
				profile.is_some(),
			)
		};
		self.name
			.update(cx, |state, cx| state.set_value(name, window, cx));
		self.bio.update(cx, |state, cx| state.set_value(bio, window, cx));
		self.pronouns
			.update(cx, |state, cx| state.set_value(pronouns, window, cx));
		self.prefilled = loaded;
		cx.notify();
	}

	fn select_guild(&mut self, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		self.guild = Some(guild);
		self.server_avatar = None;
		let nick = {
			let data = self.data.read(cx);
			let state = data.state();
			state
				.current_user_id()
				.and_then(|me| state.guild(guild)?.members.get(&me)?.nick.clone())
				.unwrap_or_default()
		};
		self.nick
			.update(cx, |state, cx| state.set_value(nick, window, cx));
		self.server_bio
			.update(cx, |state, cx| state.set_value("", window, cx));
		cx.notify();
	}

	fn pick(&mut self, slot: Slot, window: &mut Window, cx: &mut Context<Self>) {
		let filter = cx.t("profiles-image-filter").to_string();
		cx.spawn_in(window, async move |this, cx| {
			let Some(handle) = rfd::AsyncFileDialog::new()
				.add_filter(filter, &["png", "jpg", "jpeg", "gif", "webp"])
				.pick_file()
				.await
			else {
				return;
			};
			let path = handle.path().to_path_buf();
			let prepared = cx
				.background_executor()
				.spawn(async move {
					let bytes = std::fs::read(path)
						.inspect_err(|error| tracing::warn!(%error, "could not read the picked image"))
						.ok()?;
					prepare(&bytes, slot)
				})
				.await;
			let Some((png, rgba)) = prepared else {
				return;
			};
			let picked = Picked {
				png,
				preview: images::from_rgba(rgba),
			};
			this.update(cx, |this, cx| {
				match slot {
					Slot::Avatar => this.avatar = Some(picked),
					Slot::Banner => this.banner = Some(picked),
					Slot::ServerAvatar => this.server_avatar = Some(picked),
				}
				cx.notify();
			})
			.ok();
		})
		.detach();
	}

	fn save(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		match self.tab {
			Tab::User => data.send(Command::EditProfile {
				global_name: Some(self.name.read(cx).value().trim().to_owned()),
				bio: Some(self.bio.read(cx).value().to_string()),
				avatar_png: self.avatar.take().map(|picked| picked.png),
				banner_png: self.banner.take().map(|picked| picked.png),
				pronouns: Some(self.pronouns.read(cx).value().trim().to_owned()),
			}),
			Tab::Server => {
				if let Some(guild_id) = self.guild {
					data.send(Command::SetGuildProfile {
						guild_id,
						nick: Some(self.nick.read(cx).value().trim().to_owned()),
						avatar_png: self.server_avatar.take().map(|picked| picked.png),
						bio: Some(self.server_bio.read(cx).value().to_string()),
					});
				}
			}
		}
		cx.notify();
	}

	fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.avatar = None;
		self.banner = None;
		self.server_avatar = None;
		match (self.tab, self.guild) {
			(Tab::Server, Some(guild)) => self.select_guild(guild, window, cx),
			_ => self.prefill(window, cx),
		}
	}

	fn field(&self, label: SharedString, body: impl IntoElement, theme: &Theme) -> AnyElement {
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(heading(label, theme))
			.child(body)
			.into_any_element()
	}

	fn tab_button(&self, tab: Tab, key: &'static str, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let selected = self.tab == tab;
		div()
			.id(key)
			.px(theme.space_4)
			.pb(theme.space_8)
			.cursor_pointer()
			.text_size(theme.font_md)
			.font_weight(FontWeight::MEDIUM)
			.border_b_2()
			.border_color(if selected {
				theme.brand
			} else {
				gpui::transparent_black()
			})
			.text_color(if selected {
				theme.interactive_active
			} else {
				theme.interactive_normal
			})
			.on_click(cx.listener(move |this, _, _, cx| {
				this.tab = tab;
				cx.notify();
			}))
			.child(cx.t(key))
			.into_any_element()
	}

	fn upload_button(
		&self,
		id: &'static str,
		key: &'static str,
		slot: Slot,
		picked: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		div()
			.flex()
			.items_center()
			.gap(cx.global::<Theme>().space_12)
			.child(
				Button::new(id)
					.small()
					.label(cx.t(key))
					.on_click(cx.listener(move |this, _, window, cx| this.pick(slot, window, cx))),
			)
			.when(picked, |row| {
				row.child(
					div()
						.text_size(cx.global::<Theme>().font_sm)
						.text_color(cx.global::<Theme>().text_muted)
						.child(cx.t("profiles-image-ready")),
				)
			})
			.into_any_element()
	}

	fn user_form(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
		vec![
			self.field(cx.t("profiles-display-name"), Input::new(&self.name), theme),
			self.field(cx.t("profiles-pronouns"), Input::new(&self.pronouns), theme),
			self.field(
				cx.t("profiles-avatar"),
				self.upload_button(
					"profiles-avatar-pick",
					"profiles-change-avatar",
					Slot::Avatar,
					self.avatar.is_some(),
					cx,
				),
				theme,
			),
			self.field(
				cx.t("profiles-banner"),
				self.upload_button(
					"profiles-banner-pick",
					"profiles-change-banner",
					Slot::Banner,
					self.banner.is_some(),
					cx,
				),
				theme,
			),
			self.field(cx.t("profile-about-me"), Textarea::new(&self.bio), theme),
		]
	}

	fn server_form(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
		let guilds: Vec<(Snowflake, SharedString)> = {
			let data = self.data.read(cx);
			data.state()
				.guild_summaries()
				.into_iter()
				.map(|summary| (summary.id, summary.name.into()))
				.collect()
		};
		let chips = div()
			.flex()
			.flex_wrap()
			.gap(theme.space_8)
			.children(guilds.into_iter().map(|(id, name)| {
				let selected = self.guild == Some(id);
				div()
					.id(("profiles-guild", id.get()))
					.px(theme.space_12)
					.h(theme.avatar_sm)
					.flex()
					.items_center()
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.text_size(theme.font_sm)
					.font_weight(FontWeight::MEDIUM)
					.when(selected, |chip| {
						chip.bg(theme.brand).text_color(theme.text_on_brand)
					})
					.when(!selected, |chip| {
						chip.bg(theme.background_tertiary)
							.text_color(theme.interactive_normal)
							.hover(|style| style.bg(theme.modifier_selected))
					})
					.on_click(cx.listener(move |this, _, window, cx| this.select_guild(id, window, cx)))
					.child(name)
			}));
		let mut form = vec![self.field(cx.t("profiles-choose-server"), chips, theme)];
		if self.guild.is_some() {
			form.push(self.field(cx.t("profiles-nickname"), Input::new(&self.nick), theme));
			form.push(self.field(
				cx.t("profiles-avatar"),
				self.upload_button(
					"profiles-server-avatar-pick",
					"profiles-change-avatar",
					Slot::ServerAvatar,
					self.server_avatar.is_some(),
					cx,
				),
				theme,
			));
			form.push(self.field(cx.t("profile-about-me"), Textarea::new(&self.server_bio), theme));
		}
		form
	}

	fn preview(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let (username, display, current_avatar, accent) = {
			let data = self.data.read(cx);
			let state = data.state();
			let Some(user) = state.current_user() else {
				return div().into_any_element();
			};
			(
				SharedString::from(user.username.clone()),
				SharedString::from(state.display_name(None, user.id)),
				user_avatar(user, PREVIEW_AVATAR_PIXELS),
				user.accent_color
					.and_then(|color| u32::try_from(color).ok())
					.map_or(theme.brand, role_color),
			)
		};
		let server = self.tab == Tab::Server;
		let typed_name = if server {
			self.nick.read(cx).value()
		} else {
			self.name.read(cx).value()
		};
		let name = if typed_name.trim().is_empty() {
			display
		} else {
			typed_name
		};
		let bio = if server {
			self.server_bio.read(cx).value()
		} else {
			self.bio.read(cx).value()
		};
		let bio: String = bio.chars().take(BIO_LIMIT).collect();
		let pronouns = self.pronouns.read(cx).value();
		let handle: SharedString = if !server && !pronouns.trim().is_empty() {
			format!("{username} \u{b7} {}", pronouns.trim()).into()
		} else {
			username
		};
		let picked_avatar = if server { &self.server_avatar } else { &self.avatar };
		let cutout = theme.background_floating;
		let banner = div()
			.h(theme.profile_banner_height)
			.w_full()
			.bg(accent)
			.overflow_hidden()
			.children(self.banner.as_ref().filter(|_| !server).map(|picked| {
				img(picked.preview.clone())
					.size_full()
					.object_fit(ObjectFit::Cover)
			}));
		let avatar_element: AnyElement = match picked_avatar {
			Some(picked) => div()
				.size(theme.avatar_lg)
				.rounded(theme.round(theme.avatar_lg))
				.overflow_hidden()
				.child(
					img(picked.preview.clone())
						.size_full()
						.object_fit(ObjectFit::Cover),
				)
				.into_any_element(),
			None => Avatar::new(name.clone())
				.size(theme.avatar_lg)
				.url(current_avatar)
				.cutout(cutout)
				.into_any_element(),
		};
		div()
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.profile_popover_width)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.overflow_hidden()
			.child(banner)
			.child(
				div()
					.px(theme.space_16)
					.mt(-(theme.avatar_lg / 2.))
					.child(avatar_ring(theme, cutout, theme.avatar_lg, theme.space_4).child(avatar_element)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_12)
					.p(theme.space_16)
					.child(
						div()
							.flex()
							.flex_col()
							.child(
								div()
									.text_size(theme.font_lg)
									.font_weight(FontWeight::BOLD)
									.text_color(theme.header_primary)
									.child(name),
							)
							.child(
								div()
									.text_size(theme.font_sm)
									.text_color(theme.header_secondary)
									.child(handle),
							),
					)
					.when(!bio.is_empty(), |card| {
						card.child(
							div()
								.flex()
								.flex_col()
								.gap(theme.space_4)
								.pt(theme.space_12)
								.border_t_1()
								.border_color(with_alpha(theme.divider, 0.8))
								.child(heading(cx.t("profile-about-me"), theme))
								.child(markdown_text(&bio, theme)),
						)
					}),
			)
			.into_any_element()
	}
}

impl Render for ProfilesPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let form = match self.tab {
			Tab::User => self.user_form(&theme, cx),
			Tab::Server => self.server_form(&theme, cx),
		};
		let preview = self.preview(&theme, cx);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.gap(theme.space_24)
					.border_b_1()
					.border_color(theme.divider)
					.child(self.tab_button(Tab::User, "profiles-tab-user", &theme, cx))
					.child(self.tab_button(Tab::Server, "profiles-tab-server", &theme, cx)),
			)
			.child(
				div()
					.flex()
					.items_start()
					.gap(theme.space_24)
					.child(
						div()
							.flex()
							.flex_col()
							.flex_1()
							.min_w_0()
							.gap(theme.space_16)
							.children(form)
							.when(self.tab == Tab::User || self.guild.is_some(), |column| {
								column.child(
									div()
										.flex()
										.gap(theme.space_8)
										.child(
											Button::new("profiles-save")
												.primary()
												.label(cx.t("profiles-save"))
												.on_click(cx.listener(|this, _, _, cx| this.save(cx))),
										)
										.child(
											Button::new("profiles-reset")
												.ghost()
												.label(cx.t("profiles-reset"))
												.on_click(
													cx.listener(|this, _, window, cx| this.reset(window, cx)),
												),
										),
								)
							}),
					)
					.child(
						div()
							.flex()
							.flex_col()
							.gap(theme.space_8)
							.child(heading(cx.t("profiles-preview"), &theme))
							.child(preview),
					),
			)
	}
}
