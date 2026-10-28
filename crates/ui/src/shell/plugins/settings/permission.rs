use std::collections::BTreeSet;
use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use plugin_host::{Hook, Permission, PermissionRequest, Tier, Trust};

use super::widgets::{chip, note, section_label};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const DIALOG_WIDTH: f32 = 520.;
const CHEVRON: f32 = 14.;

pub struct PromptSpec {
	pub name: String,
	pub version: String,
	pub permissions: Vec<PermissionRequest>,
	pub hosts: Vec<String>,
	pub image_hosts: Vec<String>,
	pub hooks: Vec<Hook>,
	pub url_schemes: Vec<String>,
	pub trust: Option<Trust>,
}

pub fn hooks_line(hooks: &[Hook], cx: &App) -> String {
	let list = hooks
		.iter()
		.map(|hook| hook.as_str())
		.collect::<Vec<_>>()
		.join(", ");
	cx.t_args("perm-hooks", &[("list", ArgValue::from(list))])
		.to_string()
}

pub struct Decision {
	pub permissions: Vec<Permission>,
	pub hosts: Vec<String>,
}

pub type OnDecide = Rc<dyn Fn(Decision, &mut Window, &mut App)>;

pub fn trust_key(trust: Trust) -> &'static str {
	match trust {
		Trust::Unsigned => "plugin-trust-unsigned",
		Trust::Signed => "plugin-trust-signed",
		Trust::Catalog => "plugin-trust-catalog",
	}
}

pub fn trust_color(trust: Trust, theme: &Theme) -> gpui::Hsla {
	match trust {
		Trust::Unsigned => theme.warning,
		Trust::Signed => theme.link,
		Trust::Catalog => theme.success,
	}
}

pub fn open(spec: PromptSpec, on_decide: OnDecide, window: &mut Window, cx: &mut App) {
	let title = cx.t_args("perm-title", &[("name", ArgValue::from(spec.name.clone()))]);
	let prompt = cx.new(|_| PermissionPrompt::new(spec, on_decide));
	window.open_dialog(cx, move |dialog, window, _| {
		dialog
			.title(title.clone())
			.w(rem(DIALOG_WIDTH).to_pixels(window.rem_size()))
			.close_button(false)
			.child(prompt.clone())
	});
}

struct PermissionPrompt {
	spec: PromptSpec,
	choosing: bool,
	show_low: bool,
	allowed: BTreeSet<Permission>,
	allowed_hosts: BTreeSet<String>,
	on_decide: OnDecide,
}

impl PermissionPrompt {
	fn new(spec: PromptSpec, on_decide: OnDecide) -> Self {
		let allowed = spec
			.permissions
			.iter()
			.map(|request| request.permission)
			.collect();
		let allowed_hosts = spec.hosts.iter().chain(&spec.image_hosts).cloned().collect();
		Self {
			spec,
			choosing: false,
			show_low: false,
			allowed,
			allowed_hosts,
			on_decide,
		}
	}

	fn decide(&self, everything: bool, window: &mut Window, cx: &mut App) {
		let permissions = self
			.spec
			.permissions
			.iter()
			.map(|request| request.permission)
			.filter(|permission| everything || self.allowed.contains(permission))
			.collect();
		let hosts = self
			.spec
			.hosts
			.iter()
			.chain(&self.spec.image_hosts)
			.filter(|host| everything || self.allowed_hosts.contains(*host))
			.cloned()
			.collect();
		(self.on_decide)(Decision { permissions, hosts }, window, cx);
		window.close_dialog(cx);
	}

	fn permission_row(
		&self,
		request: &PermissionRequest,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let permission = request.permission;
		let warning = match permission.tier() {
			Tier::Sensitive => Some((cx.t("perm-sensitive"), theme.danger)),
			Tier::High => Some((cx.t("perm-high"), theme.warning)),
			Tier::Low | Tier::Medium => None,
		};
		div()
			.flex()
			.items_start()
			.gap(theme.space_12)
			.when(self.choosing, |row| {
				row.child(
					Checkbox::new(("perm-check", permission as usize))
						.checked(self.allowed.contains(&permission))
						.on_click(cx.listener(move |this, checked: &bool, _, cx| {
							if *checked {
								this.allowed.insert(permission);
							} else {
								this.allowed.remove(&permission);
							}
							cx.notify();
						})),
				)
			})
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.gap(theme.space_2)
					.child(
						div()
							.flex()
							.items_center()
							.gap(theme.space_8)
							.child(
								div()
									.text_size(theme.font_sm)
									.font_weight(FontWeight::MEDIUM)
									.text_color(theme.text_normal)
									.child(cx.t(&format!("perm-{}", permission.as_str()))),
							)
							.when_some(warning, |line, (label, color)| {
								line.child(chip(label, color, theme))
							}),
					)
					.when(!request.reason.is_empty(), |column| {
						column.child(
							div()
								.text_size(theme.font_sm)
								.italic()
								.text_color(theme.text_muted)
								.child(request.reason.clone()),
						)
					})
					.child(
						div()
							.font_family(theme.code_font.clone())
							.text_size(theme.font_tiny)
							.text_color(theme.text_muted)
							.child(permission.as_str()),
					),
			)
			.into_any_element()
	}

	fn host_row(&self, host: &str, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let owned = host.to_owned();
		div()
			.flex()
			.items_center()
			.gap(theme.space_12)
			.when(self.choosing, |row| {
				row.child(
					Checkbox::new(gpui::SharedString::from(format!("perm-host-{host}")))
						.checked(self.allowed_hosts.contains(host))
						.on_click(cx.listener(move |this, checked: &bool, _, cx| {
							if *checked {
								this.allowed_hosts.insert(owned.clone());
							} else {
								this.allowed_hosts.remove(&owned);
							}
							cx.notify();
						})),
				)
			})
			.child(
				div()
					.font_family(theme.code_font.clone())
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.child(host.to_owned()),
			)
			.into_any_element()
	}

	fn low_summary(&self, count: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let open = self.show_low || self.choosing;
		div()
			.id("perm-low")
			.flex()
			.items_center()
			.gap(theme.space_8)
			.cursor_pointer()
			.text_size(theme.font_sm)
			.text_color(theme.text_muted)
			.on_click(cx.listener(|this, _, _, cx| {
				this.show_low = !this.show_low;
				cx.notify();
			}))
			.child(
				Icon::new(if open {
					IconKind::ChevronDown
				} else {
					IconKind::ChevronRight
				})
				.size(rem(CHEVRON))
				.color(theme.text_muted),
			)
			.child(cx.t_args("perm-low-summary", &[("count", ArgValue::from(count))]))
			.into_any_element()
	}

	fn footer(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let asks = !self.spec.permissions.is_empty()
			|| !self.spec.hosts.is_empty()
			|| !self.spec.image_hosts.is_empty();
		let cancel = Button::new("perm-cancel")
			.label(cx.t("action-cancel"))
			.on_click(|_, window, cx| window.close_dialog(cx));
		let mut row = div()
			.flex()
			.items_center()
			.justify_end()
			.gap(theme.space_8)
			.child(cancel);
		if !asks {
			row = row.child(
				Button::new("perm-install")
					.primary()
					.label(cx.t("perm-install"))
					.on_click(cx.listener(|this, _, window, cx| this.decide(true, window, cx))),
			);
		} else if self.choosing {
			row = row
				.child(
					Button::new("perm-back")
						.label(cx.t("perm-back"))
						.on_click(cx.listener(|this, _, _, cx| {
							this.choosing = false;
							cx.notify();
						})),
				)
				.child(
					Button::new("perm-allow-selected")
						.primary()
						.label(cx.t("perm-allow-selected"))
						.on_click(cx.listener(|this, _, window, cx| this.decide(false, window, cx))),
				);
		} else {
			row = row
				.child(
					Button::new("perm-choose")
						.label(cx.t("perm-choose"))
						.on_click(cx.listener(|this, _, _, cx| {
							this.choosing = true;
							cx.notify();
						})),
				)
				.child(
					Button::new("perm-allow-all")
						.primary()
						.label(cx.t("perm-allow-all"))
						.on_click(cx.listener(|this, _, window, cx| this.decide(true, window, cx))),
				);
		}
		row.into_any_element()
	}
}

impl Render for PermissionPrompt {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let mut ordered: Vec<&PermissionRequest> = self.spec.permissions.iter().collect();
		ordered.sort_by_key(|request| std::cmp::Reverse(request.permission.tier()));
		let (low, raised): (Vec<&PermissionRequest>, Vec<&PermissionRequest>) = ordered
			.into_iter()
			.partition(|request| request.permission.tier() == Tier::Low);
		let mut rows: Vec<AnyElement> = raised
			.into_iter()
			.map(|request| self.permission_row(request, &theme, cx))
			.collect();
		if !low.is_empty() {
			rows.push(self.low_summary(low.len(), &theme, cx));
			if self.show_low || self.choosing {
				rows.extend(
					low.into_iter()
						.map(|request| self.permission_row(request, &theme, cx)),
				);
			}
		}
		let hosts: Vec<AnyElement> = self
			.spec
			.hosts
			.iter()
			.map(|host| self.host_row(host, &theme, cx))
			.collect();
		let image_hosts: Vec<AnyElement> = self
			.spec
			.image_hosts
			.iter()
			.map(|host| self.host_row(host, &theme, cx))
			.collect();
		let footer = self.footer(&theme, cx);
		let hooks = (!self.spec.hooks.is_empty()).then(|| note(hooks_line(&self.spec.hooks, cx), &theme));
		let schemes: Vec<AnyElement> = self
			.spec
			.url_schemes
			.iter()
			.map(|scheme| {
				div()
					.font_family(theme.code_font.clone())
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.child(format!("{scheme}://"))
					.into_any_element()
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(note(
						cx.t_args(
							"perm-version",
							&[("version", ArgValue::from(self.spec.version.clone()))],
						),
						&theme,
					))
					.children(
						self.spec
							.trust
							.map(|trust| chip(cx.t(trust_key(trust)), trust_color(trust, &theme), &theme)),
					),
			)
			.child(if rows.is_empty() && hosts.is_empty() && image_hosts.is_empty() {
				note(cx.t("perm-none"), &theme)
			} else {
				div()
					.flex()
					.flex_col()
					.gap(theme.space_12)
					.child(section_label("perm-requests", &theme, cx))
					.children(rows)
					.into_any_element()
			})
			.when(!hosts.is_empty(), |body| {
				body.child(
					div()
						.flex()
						.flex_col()
						.gap(theme.space_8)
						.child(section_label("perm-hosts", &theme, cx))
						.children(hosts),
				)
			})
			.when(!schemes.is_empty(), |body| {
				body.child(
					div()
						.flex()
						.flex_col()
						.gap(theme.space_8)
						.child(section_label("perm-url-schemes", &theme, cx))
						.children(schemes),
				)
			})
			.children(hooks)
			.when(!image_hosts.is_empty(), |body| {
				body.child(
					div()
						.flex()
						.flex_col()
						.gap(theme.space_8)
						.child(section_label("perm-image-hosts", &theme, cx))
						.children(image_hosts),
				)
			})
			.child(footer)
	}
}
