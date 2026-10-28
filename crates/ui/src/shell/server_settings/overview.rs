use gpui::{
	AppContext as _, Context, Entity, IntoElement, ObjectFit, ParentElement as _, Render, SharedString,
	Styled as _, StyledImage as _, Subscription, Window, div, img, prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use session::Command;

use super::kit;
use crate::components::{Avatar, field, hint};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::community::files::{Fit, IMAGE_TYPES, Picked, pick_image};
use crate::theme::Theme;

const ICON_SIDE: u32 = 512;
const NAME_LIMIT: usize = 100;

pub struct OverviewPage {
	data: Entity<Data>,
	guild: Snowflake,
	name: Entity<InputState>,
	description: Entity<TextareaState>,
	initial_name: String,
	initial_description: String,
	icon: Option<Picked>,
	_subscriptions: Vec<Subscription>,
}

impl OverviewPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let (initial_name, initial_description) = {
			let state = data.read(cx).state();
			state
				.guild(guild)
				.map(|found| (found.name.clone(), found.description.clone().unwrap_or_default()))
				.unwrap_or_default()
		};
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(initial_name.clone(), window, cx);
			state
		});
		let placeholder = cx.t("server-overview-description-placeholder");
		let description = cx.new(|cx| {
			let mut state = TextareaState::new(window, cx)
				.auto_grow(3, 8)
				.placeholder(placeholder);
			state.set_value(initial_description.clone(), window, cx);
			state
		});
		let subscriptions = vec![
			cx.subscribe_in(&name, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
			cx.subscribe_in(&description, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
		];
		Self {
			data,
			guild,
			name,
			description,
			initial_name,
			initial_description,
			icon: None,
			_subscriptions: subscriptions,
		}
	}

	fn current_name(&self, cx: &gpui::App) -> String {
		self.name
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(NAME_LIMIT)
			.collect()
	}

	fn current_description(&self, cx: &gpui::App) -> String {
		self.description.read(cx).value().trim().to_owned()
	}

	fn dirty(&self, cx: &gpui::App) -> bool {
		self.icon.is_some()
			|| (self.current_name(cx) != self.initial_name && !self.current_name(cx).is_empty())
			|| self.current_description(cx) != self.initial_description
	}

	fn save(&mut self, cx: &mut Context<Self>) {
		let name = self.current_name(cx);
		let description = self.current_description(cx);
		self.data.read(cx).send(Command::EditGuild {
			guild_id: self.guild,
			name: (name != self.initial_name && !name.is_empty()).then(|| name.clone()),
			icon_png: self.icon.take().map(|picked| picked.bytes),
			description: (description != self.initial_description).then(|| description.clone()),
		});
		if !name.is_empty() {
			self.initial_name = name;
		}
		self.initial_description = description;
		cx.notify();
	}

	fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let name = self.initial_name.clone();
		let description = self.initial_description.clone();
		self.name
			.update(cx, |state, cx| state.set_value(name, window, cx));
		self.description
			.update(cx, |state, cx| state.set_value(description, window, cx));
		self.icon = None;
		cx.notify();
	}
}

impl Render for OverviewPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (guild_name, icon_url) = {
			let state = self.data.read(cx).state();
			state
				.guild(self.guild)
				.map(|found| {
					(
						found.name.clone(),
						found
							.icon
							.as_deref()
							.map(|hash| media::cdn::guild_icon_url(self.guild.get(), hash, 256)),
					)
				})
				.unwrap_or_default()
		};
		let dirty = self.dirty(cx);
		let weak_reset = cx.weak_entity();
		let weak_save = cx.weak_entity();
		let preview: SharedString = guild_name.clone().into();
		let tile = div()
			.size(theme.portrait_lg * 2.)
			.flex_none()
			.rounded_full()
			.overflow_hidden()
			.child(match (&self.icon, icon_url) {
				(Some(picked), _) => img(picked.preview.clone())
					.size_full()
					.object_fit(ObjectFit::Cover)
					.into_any_element(),
				(None, url) => {
					let avatar = Avatar::new(preview).size(theme.portrait_lg * 2.);
					match url {
						Some(url) => avatar.url(url).into_any_element(),
						None => avatar.into_any_element(),
					}
				}
			});
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-overview"), &theme))
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_24)
					.pb(theme.space_32)
					.child(
						div().flex().items_center().gap(theme.space_24).child(tile).child(
							div()
								.flex()
								.flex_col()
								.items_start()
								.gap(theme.space_8)
								.child(
									Button::new("overview-icon")
										.primary()
										.label(cx.t("server-overview-upload-icon"))
										.on_click(cx.listener(|_, _, _, cx| {
											pick_image(
												cx,
												Fit::Square(ICON_SIDE),
												IMAGE_TYPES,
												|this: &mut Self, picked, cx| {
													this.icon = Some(picked);
													cx.notify();
												},
											)
										})),
								)
								.child(hint(cx.t("server-overview-icon-hint"), &theme)),
						),
					)
					.child(field(
						cx.t("server-overview-name"),
						&theme,
						Input::new(&self.name),
					))
					.child(field(
						cx.t("server-overview-description"),
						&theme,
						div()
							.rounded(theme.radius_sm)
							.bg(theme.input_background)
							.px(theme.space_12)
							.py(theme.space_8)
							.child(
								Textarea::new(&self.description)
									.appearance(false)
									.bordered(false)
									.p_0(),
							),
					))
					.overflow_y_scrollbar()
					.id("overview-scroll")
					.flex_1()
					.min_h_0(),
			)
			.when(dirty, |page| {
				page.child(kit::save_bar(
					&theme,
					cx,
					move |window, cx| {
						weak_reset.update(cx, |this, cx| this.reset(window, cx)).ok();
					},
					move |_, cx| {
						weak_save.update(cx, |this, cx| this.save(cx)).ok();
					},
				))
			})
	}
}
