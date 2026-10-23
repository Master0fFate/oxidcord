use std::collections::HashSet;
use std::rc::Rc;

use gpui::{
	AnyElement, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement, ObjectFit,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use media::cdn::{ProxyFormat, attachment_proxy};
use protocol::{Attachment, ForumLayout, ForumSortOrder, ForumTag, Snowflake};
use session::Command;
use state::{ForumPost, State};

use crate::components::{Avatar, IconButton, PickHandler, badge, select};
use crate::data::Data;
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::lookup::avatar_for;
use crate::shell::community::art::cover;
use crate::shell::message::snippet;
use crate::theme::{Theme, rem, with_alpha};

const EXCERPT_LENGTH: usize = 180;
const STARTER_LIMIT: usize = 36;
const THUMB_PIXELS: f32 = 320.;

pub enum ForumEvent {
	OpenPost(Snowflake),
	NewPost(Snowflake),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
	Activity,
	Creation,
}

struct Card {
	id: Snowflake,
	title: SharedString,
	excerpt: SharedString,
	thumbnail: Option<String>,
	tags: Vec<(SharedString, bool)>,
	author: SharedString,
	avatar: String,
	replies: i64,
	activity: SharedString,
	pinned: bool,
	archived: bool,
}

pub struct ForumView {
	data: Entity<Data>,
	guild: Option<Snowflake>,
	channel: Option<Snowflake>,
	gallery: bool,
	sort: Sort,
	filter: HashSet<Snowflake>,
	requested: HashSet<Snowflake>,
}

impl EventEmitter<ForumEvent> for ForumView {}

fn first_image(attachments: &[Attachment]) -> Option<String> {
	attachments.iter().find_map(|attachment| {
		let image = attachment
			.content_type
			.as_deref()
			.is_some_and(|kind| kind.starts_with("image/"));
		let url = attachment.url.as_deref()?;
		image.then(|| attachment_proxy(url, THUMB_PIXELS as u32, THUMB_PIXELS as u32, ProxyFormat::Webp))
	})
}

fn tag_label(tag: &ForumTag) -> SharedString {
	match tag.emoji_name.as_deref().filter(|emoji| !emoji.is_empty()) {
		Some(emoji) => format!("{emoji} {}", tag.name).into(),
		None => tag.name.clone().into(),
	}
}

impl ForumView {
	pub fn new(data: Entity<Data>) -> Self {
		Self {
			data,
			guild: None,
			channel: None,
			gallery: false,
			sort: Sort::Activity,
			filter: HashSet::new(),
			requested: HashSet::new(),
		}
	}

	pub fn channel(&self) -> Option<Snowflake> {
		self.channel
	}

	pub fn show(&mut self, guild: Option<Snowflake>, channel: Option<Snowflake>, cx: &mut Context<Self>) {
		if self.channel == channel && self.guild == guild {
			return;
		}
		self.guild = guild;
		self.channel = channel;
		self.filter.clear();
		self.requested.clear();
		if let (Some(guild), Some(channel)) = (guild, channel) {
			let (gallery, sort) = {
				let data = self.data.read(cx);
				let state = data.state();
				let found = state.channel(channel);
				(
					found.is_some_and(|forum| {
						forum.default_forum_layout == Some(ForumLayout::GalleryView)
							|| forum.kind == protocol::ChannelType::GuildMedia
					}),
					if found
						.is_some_and(|forum| forum.default_sort_order == Some(ForumSortOrder::CreationDate))
					{
						Sort::Creation
					} else {
						Sort::Activity
					},
				)
			};
			self.gallery = gallery;
			self.sort = sort;
			let data = self.data.read(cx);
			data.send(Command::LoadActiveThreads { guild_id: guild });
			data.send(Command::LoadArchivedThreads { channel_id: channel });
		}
		self.request_starters(cx);
		cx.notify();
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		self.request_starters(cx);
		cx.notify();
	}

	fn visible_posts(&self, state: &State, channel: Snowflake) -> Vec<ForumPost> {
		let mut posts: Vec<ForumPost> = state
			.forum_posts(channel)
			.into_iter()
			.filter(|post| self.filter.is_empty() || post.tags.iter().any(|tag| self.filter.contains(tag)))
			.collect();
		posts.sort_by_key(|post| {
			let key = match self.sort {
				Sort::Activity => post.last_activity,
				Sort::Creation => post.created_at,
			};
			(
				std::cmp::Reverse(post.pinned),
				std::cmp::Reverse(key),
				std::cmp::Reverse(post.id),
			)
		});
		posts
	}

	fn request_starters(&mut self, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let wanted: Vec<Snowflake> = {
			let data = self.data.read(cx);
			let state = data.state();
			self.visible_posts(&state, channel)
				.into_iter()
				.take(STARTER_LIMIT)
				.filter(|post| state.messages(post.id).is_none())
				.map(|post| post.id)
				.filter(|id| !self.requested.contains(id))
				.collect()
		};
		for id in wanted {
			self.requested.insert(id);
			self.data.read(cx).send(Command::LoadAround {
				channel_id: id,
				message_id: id,
			});
		}
	}

	fn card(&self, post: &ForumPost, tags: &[ForumTag], state: &State, cx: &gpui::App) -> Card {
		let starter = state.forum_starter(post.id).cloned();
		let excerpt: SharedString = starter
			.as_ref()
			.map(|message| snippet(&markdown::parse(&message.content), EXCERPT_LENGTH))
			.unwrap_or_default()
			.into();
		let thumbnail = starter
			.as_ref()
			.and_then(|message| first_image(&message.attachments));
		let owner = post.owner_id;
		Card {
			id: post.id,
			title: post.name.clone().into(),
			excerpt,
			thumbnail,
			tags: tags
				.iter()
				.filter(|tag| post.tags.contains(&tag.id))
				.map(|tag| (tag_label(tag), tag.moderated))
				.collect(),
			author: owner
				.map(|owner| state.display_name(self.guild, owner))
				.unwrap_or_default()
				.into(),
			avatar: owner.map_or_else(String::new, |owner| avatar_for(state, owner, 48)),
			replies: (post.message_count - 1).max(0),
			activity: format::message_time(post.last_activity, cx),
			pinned: post.pinned,
			archived: post.archived,
		}
	}

	fn meta(&self, card: &Card, theme: &Theme, cx: &gpui::App) -> AnyElement {
		div()
			.flex()
			.flex_wrap()
			.items_center()
			.gap(theme.space_8)
			.text_size(theme.font_sm)
			.text_color(theme.text_muted)
			.child(
				Avatar::new(card.author.clone())
					.size(theme.portrait_sm)
					.url(card.avatar.clone()),
			)
			.child(
				div()
					.flex_none()
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_secondary)
					.child(card.author.clone()),
			)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_4)
					.child(
						Icon::new(IconKind::Chat)
							.size(theme.glyph_sm)
							.color(theme.text_muted),
					)
					.child(cx.t_args("forum-replies", &[("count", ArgValue::from(card.replies))])),
			)
			.child(div().truncate().child(card.activity.clone()))
			.into_any_element()
	}

	fn tag_row(&self, card: &Card, theme: &Theme) -> AnyElement {
		div()
			.flex()
			.flex_wrap()
			.gap(theme.space_4)
			.children(card.tags.iter().map(|(label, moderated)| {
				badge(
					label.clone(),
					if *moderated {
						theme.brand
					} else {
						theme.interactive_normal
					},
					theme,
				)
			}))
			.into_any_element()
	}

	fn list_card(&self, card: &Card, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = card.id;
		div()
			.id(("forum-post", id.get()))
			.flex()
			.gap(theme.space_16)
			.mx(theme.space_16)
			.mb(theme.space_8)
			.p(theme.space_16)
			.rounded(theme.radius_md)
			.bg(theme.background_secondary)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_selected))
			.when(card.archived, |row| row.opacity(0.7))
			.on_click(cx.listener(move |_, _, _, cx| cx.emit(ForumEvent::OpenPost(id))))
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.gap(theme.space_8)
					.child(self.title_row(card, theme))
					.when(!card.excerpt.is_empty(), |column| {
						column.child(
							div()
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.line_clamp(2)
								.child(card.excerpt.clone()),
						)
					})
					.when(!card.tags.is_empty(), |column| {
						column.child(self.tag_row(card, theme))
					})
					.child(self.meta(card, theme, cx)),
			)
			.children(card.thumbnail.as_ref().map(|url| {
				div()
					.flex_none()
					.w(theme.forum_thumb_width)
					.h(theme.forum_thumb_width)
					.rounded(theme.radius_sm)
					.overflow_hidden()
					.child(cover(
						url,
						theme.forum_thumb_width,
						theme.forum_thumb_width,
						ObjectFit::Cover,
						cx,
					))
			}))
			.into_any_element()
	}

	fn gallery_card(&self, card: &Card, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = card.id;
		div()
			.id(("forum-post", id.get()))
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.forum_card_width)
			.rounded(theme.radius_md)
			.overflow_hidden()
			.bg(theme.background_secondary)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_selected))
			.when(card.archived, |column| column.opacity(0.7))
			.on_click(cx.listener(move |_, _, _, cx| cx.emit(ForumEvent::OpenPost(id))))
			.child(
				div()
					.w_full()
					.h(theme.forum_thumb_height)
					.bg(theme.background_tertiary)
					.flex()
					.items_center()
					.justify_center()
					.child(match &card.thumbnail {
						Some(url) => cover(
							url,
							theme.forum_card_width,
							theme.forum_thumb_height,
							ObjectFit::Cover,
							cx,
						),
						None => Icon::new(IconKind::Image)
							.size(theme.portrait_lg)
							.color(theme.interactive_muted)
							.into_any_element(),
					}),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.p(theme.space_12)
					.child(self.title_row(card, theme))
					.when(!card.tags.is_empty(), |column| {
						column.child(self.tag_row(card, theme))
					})
					.child(self.meta(card, theme, cx)),
			)
			.into_any_element()
	}

	fn title_row(&self, card: &Card, theme: &Theme) -> AnyElement {
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.when(card.pinned, |row| {
				row.child(Icon::new(IconKind::Pin).size(theme.glyph_sm).color(theme.brand))
			})
			.child(
				div()
					.min_w_0()
					.truncate()
					.text_size(theme.font_md)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(card.title.clone()),
			)
			.into_any_element()
	}

	fn toolbar(
		&self,
		channel: Snowflake,
		tags: &[ForumTag],
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let weak = cx.weak_entity();
		let labels = [cx.t("forum-sort-activity"), cx.t("forum-sort-creation")];
		let selected = usize::from(self.sort == Sort::Creation);
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.sort = if index == 0 {
					Sort::Activity
				} else {
					Sort::Creation
				};
				cx.notify();
			})
			.ok();
		});
		let chips: Vec<AnyElement> = tags
			.iter()
			.map(|tag| {
				let id = tag.id;
				let active = self.filter.contains(&id);
				div()
					.id(("forum-tag", id.get()))
					.flex()
					.items_center()
					.h(rem(28.))
					.px(theme.space_12)
					.rounded_full()
					.cursor_pointer()
					.border_1()
					.border_color(if active { theme.brand } else { theme.divider })
					.bg(if active {
						with_alpha(theme.brand, 0.2)
					} else {
						theme.background_secondary
					})
					.text_size(theme.font_sm)
					.text_color(if active {
						theme.interactive_active
					} else {
						theme.interactive_normal
					})
					.hover(|style| style.bg(theme.modifier_selected))
					.on_click(cx.listener(move |this, _, _, cx| {
						if !this.filter.remove(&id) {
							this.filter.insert(id);
						}
						cx.notify();
					}))
					.child(tag_label(tag))
					.into_any_element()
			})
			.collect();
		div()
			.flex()
			.flex_none()
			.flex_col()
			.gap(theme.space_12)
			.px(theme.space_16)
			.py(theme.space_12)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(
						Button::new("forum-new-post")
							.primary()
							.icon(gpui_component::Icon::default().path(IconKind::Plus.path()))
							.label(cx.t("forum-new-post"))
							.on_click(cx.listener(move |_, _, _, cx| cx.emit(ForumEvent::NewPost(channel)))),
					)
					.child(div().flex_1())
					.child(select(
						"forum-sort",
						labels[selected].clone(),
						&labels,
						selected,
						pick,
					))
					.child(
						IconButton::new("forum-layout-list", IconKind::ListLayout)
							.tooltip(cx.t("forum-layout-list"))
							.filled_hover()
							.active(!self.gallery)
							.on_click(cx.listener(|this, _, _, cx| {
								this.gallery = false;
								cx.notify();
							})),
					)
					.child(
						IconButton::new("forum-layout-gallery", IconKind::GridLayout)
							.tooltip(cx.t("forum-layout-gallery"))
							.filled_hover()
							.active(self.gallery)
							.on_click(cx.listener(|this, _, _, cx| {
								this.gallery = true;
								cx.notify();
							})),
					),
			)
			.when(!chips.is_empty(), |toolbar| {
				toolbar.child(
					div()
						.flex()
						.flex_wrap()
						.items_center()
						.gap(theme.space_8)
						.child(
							div()
								.text_size(theme.font_tiny)
								.font_weight(FontWeight::BOLD)
								.text_color(theme.header_secondary)
								.child(cx.t("forum-tags").to_uppercase()),
						)
						.children(chips),
				)
			})
			.into_any_element()
	}
}

impl Render for ForumView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let Some(channel) = self.channel else {
			return div().flex_1().into_any_element();
		};
		let (cards, tags) = {
			let data = self.data.read(cx);
			let state = data.state();
			let tags: Vec<ForumTag> = state
				.channel(channel)
				.map(|forum| forum.available_tags.to_vec())
				.unwrap_or_default();
			let cards: Vec<Card> = self
				.visible_posts(&state, channel)
				.iter()
				.map(|post| self.card(post, &tags, &state, cx))
				.collect();
			(cards, tags)
		};
		let toolbar = self.toolbar(channel, &tags, &theme, cx);
		let items: Vec<AnyElement> = cards
			.iter()
			.map(|card| {
				if self.gallery {
					self.gallery_card(card, &theme, cx)
				} else {
					self.list_card(card, &theme, cx)
				}
			})
			.collect();
		let empty = items.is_empty();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_w_0()
			.min_h_0()
			.bg(theme.background_primary)
			.child(toolbar)
			.child(
				div()
					.flex()
					.when(self.gallery, |list| {
						list.flex_row()
							.flex_wrap()
							.content_start()
							.gap(theme.space_16)
							.px(theme.space_16)
							.pb(theme.space_16)
					})
					.when(!self.gallery, |list| list.flex_col().pb(theme.space_16))
					.children(items)
					.when(empty, |list| {
						list.child(
							div()
								.flex_1()
								.flex()
								.flex_col()
								.items_center()
								.gap(theme.space_8)
								.p(theme.space_32)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(
									Icon::new(IconKind::Forum)
										.size(theme.portrait_lg)
										.color(theme.interactive_muted),
								)
								.child(cx.t("forum-empty")),
						)
					})
					.overflow_y_scrollbar()
					.id("forum-posts")
					.flex_1()
					.min_h_0(),
			)
			.into_any_element()
	}
}
