use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
	SharedString, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::switch::Switch;
use gpui_component::{Disableable as _, Sizable as _, WindowExt as _};
use plugin_host::catalog::{Catalog, CatalogEntry};
use plugin_host::{Package, PermissionRequest, PluginInfo, PluginState, Trust};
use serde_json::Value;
use session::Command;

use super::permission::{self, Decision, PromptSpec, trust_color, trust_key};
use super::plugin_form::PluginForm;
use super::widgets::{banner, card, chip, heading, icon_tile, note, section_label, tab_pill};
use crate::AppFlags;
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::shell::SettingsBackend;
use crate::shell::plugins::bridge::PluginBridge;
use crate::shell::plugins::overlays::GrantRequest;
use crate::theme::{Theme, rem};

const DEFAULT_CATALOG_URL: &str =
	"https://raw.githubusercontent.com/Master0fFate/oxidcord/main/plugins/catalog.json";
const URL_HEIGHT: f32 = 32.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
	Installed,
	Catalog,
}

pub struct PluginsPage {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	tab: Tab,
	form: Option<Entity<PluginForm>>,
	catalog_url: Entity<InputState>,
	safe_mode_setting: bool,
	notice: Option<SharedString>,
	_subscription: Subscription,
}

fn state_key(state: PluginState) -> &'static str {
	match state {
		PluginState::Loaded => "plugin-state-loaded",
		PluginState::Disabled => "plugin-state-disabled",
		PluginState::Crashed => "plugin-state-crashed",
		PluginState::Incompatible => "plugin-state-incompatible",
	}
}

fn state_color(state: PluginState, theme: &Theme) -> gpui::Hsla {
	match state {
		PluginState::Loaded => theme.success,
		PluginState::Disabled => theme.text_muted,
		PluginState::Crashed => theme.danger,
		PluginState::Incompatible => theme.warning,
	}
}

fn queue_grant(id: String, version: String, decision: &Decision, cx: &mut App) {
	let ui = cx.global::<PluginBridge>().ui().clone();
	let request = GrantRequest {
		id,
		version,
		permissions: decision.permissions.clone(),
		hosts: decision.hosts.clone(),
	};
	ui.update(cx, |ui, _| ui.queue_grant(request));
}

impl PluginsPage {
	pub fn new(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let catalog_url = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(DEFAULT_CATALOG_URL, window, cx);
			state
		});
		let subscription = cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.plugins || changes.plugin_slots)
			{
				cx.notify();
			}
		});
		let safe_mode_setting = (backend.read)().plugins.safe_mode;
		Self {
			data,
			backend,
			tab: Tab::Installed,
			form: None,
			catalog_url,
			safe_mode_setting,
			notice: None,
			_subscription: subscription,
		}
	}

	fn send(&self, command: Command, cx: &App) {
		self.data.read(cx).send(command);
	}

	fn set_enabled(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
		let command = if enabled {
			Command::PluginEnable(id.to_owned())
		} else {
			Command::PluginDisable(id.to_owned())
		};
		self.send(command, cx);
	}

	fn set_safe_mode(&mut self, safe: bool, cx: &mut Context<Self>) {
		self.safe_mode_setting = safe;
		(self.backend.write)(&move |settings| settings.plugins.safe_mode = safe);
		cx.notify();
	}

	fn pick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let filter = cx.t("plugin-file-filter").to_string();
		cx.spawn_in(window, async move |this, cx| {
			let Some(handle) = rfd::AsyncFileDialog::new()
				.add_filter(filter, &["oxidcord-plugin"])
				.pick_file()
				.await
			else {
				return;
			};
			let path = handle.path().to_path_buf();
			let opened = {
				let path = path.clone();
				cx.background_executor()
					.spawn(async move { Package::open(&path) })
					.await
			};
			this.update_in(cx, |this, window, cx| match opened {
				Ok(package) => {
					let trust = package.verify(None);
					let spec = PromptSpec {
						name: package.manifest.name.clone(),
						version: package.manifest.version.to_string(),
						permissions: package.manifest.permissions.clone(),
						hosts: package.manifest.network_hosts.clone(),
						trust: Some(trust),
					};
					this.review_install(path, package.manifest.id.clone(), spec, window, cx);
				}
				Err(error) => {
					this.notice = Some(error.to_string().into());
					cx.notify();
				}
			})
			.ok();
		})
		.detach();
	}

	fn review_install(
		&mut self,
		path: PathBuf,
		id: String,
		spec: PromptSpec,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.notice = None;
		let version = spec.version.clone();
		let data = self.data.clone();
		permission::open(
			spec,
			Rc::new(move |decision, _, cx| {
				queue_grant(id.clone(), version.clone(), &decision, cx);
				data.read(cx).send(Command::PluginInstall { path: path.clone() });
			}),
			window,
			cx,
		);
	}

	fn review_installed(&mut self, info: &PluginInfo, window: &mut Window, cx: &mut Context<Self>) {
		let spec = PromptSpec {
			name: info.name.clone(),
			version: info.version.to_string(),
			permissions: info.permissions.clone(),
			hosts: info.network_hosts.clone(),
			trust: Some(info.trust),
		};
		let id = info.id.clone();
		let data = self.data.clone();
		permission::open(
			spec,
			Rc::new(move |decision, _, cx| {
				data.read(cx).send(Command::PluginGrant {
					id: id.clone(),
					permissions: decision.permissions,
					hosts: decision.hosts,
				});
			}),
			window,
			cx,
		);
	}

	fn confirm_uninstall(&mut self, id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
		let data = self.data.clone();
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			let id = id.clone();
			alert
				.title(cx.t("plugin-uninstall-title"))
				.description(cx.t_args("plugin-uninstall-body", &[("name", ArgValue::from(name.clone()))]))
				.confirm()
				.ok_text(cx.t("plugin-uninstall"))
				.ok_variant(ButtonVariant::Danger)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::PluginUninstall(id.clone()));
					true
				})
		});
	}

	fn open_form(&mut self, id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
		let manager = Arc::clone(self.data.read(cx).session().plugins());
		cx.spawn_in(window, async move |this, cx| {
			let plugin = id.clone();
			let schema = cx
				.background_executor()
				.spawn(async move {
					let live = manager
						.settings_schema(&plugin)
						.ok()
						.and_then(|schema| serde_json::from_str::<Value>(&schema).ok());
					live.or_else(|| manager.manifest(&plugin).ok()?.settings_schema.clone())
				})
				.await;
			this.update_in(cx, |this, window, cx| {
				let form = cx.new(|cx| PluginForm::new(id, name.into(), schema, window, cx));
				this.form = Some(form);
				cx.notify();
			})
			.ok();
		})
		.detach();
	}

	fn confirm_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let url = self.catalog_url.read(cx).value().trim().to_owned();
		if url.is_empty() {
			return;
		}
		let data = self.data.clone();
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			let url = url.clone();
			alert
				.title(cx.t("catalog-consent-title"))
				.description(cx.t_args("catalog-consent-body", &[("url", ArgValue::from(url.clone()))]))
				.confirm()
				.ok_text(cx.t("catalog-refresh"))
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::CatalogRefresh { url: url.clone() });
					true
				})
		});
	}

	fn review_catalog(&mut self, entry: &CatalogEntry, window: &mut Window, cx: &mut Context<Self>) {
		let spec = PromptSpec {
			name: entry.name.clone(),
			version: entry.version.to_string(),
			permissions: entry
				.permissions
				.iter()
				.map(|permission| PermissionRequest {
					permission: *permission,
					reason: String::new(),
				})
				.collect(),
			hosts: Vec::new(),
			trust: Some(Trust::Catalog),
		};
		let (id, version) = (entry.id.clone(), entry.version.to_string());
		let data = self.data.clone();
		permission::open(
			spec,
			Rc::new(move |decision, _, cx| {
				queue_grant(id.clone(), version.clone(), &decision, cx);
				data.read(cx).send(Command::CatalogInstall { id: id.clone() });
			}),
			window,
			cx,
		);
	}

	fn plugin_card(
		&self,
		index: usize,
		info: &PluginInfo,
		safe_mode: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let author = self
			.data
			.read(cx)
			.session()
			.plugins()
			.manifest(&info.id)
			.map(|manifest| manifest.authors.join(", "))
			.ok()
			.filter(|authors| !authors.is_empty())
			.map_or_else(|| cx.t("plugin-unknown-author"), |authors| authors.into());
		let incompatible = info.state == PluginState::Incompatible;
		let toggle_id = info.id.clone();
		let reload_id = info.id.clone();
		let form_id = info.id.clone();
		let form_name = info.name.clone();
		let remove_id = info.id.clone();
		let remove_name = info.name.clone();
		let review = info.clone();
		div()
			.child(
				card(theme)
					.child(
						div()
							.flex()
							.items_start()
							.gap(theme.space_12)
							.child(icon_tile(IconKind::Plugins, theme))
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
													.text_size(theme.font_md)
													.font_weight(FontWeight::SEMIBOLD)
													.text_color(theme.header_primary)
													.child(info.name.clone()),
											)
											.child(
												div()
													.text_size(theme.font_sm)
													.text_color(theme.text_muted)
													.child(format!("v{}", info.version)),
											)
											.child(chip(
												cx.t(trust_key(info.trust)),
												trust_color(info.trust, theme),
												theme,
											))
											.child(chip(
												cx.t(state_key(info.state)),
												state_color(info.state, theme),
												theme,
											)),
									)
									.child(note(
										cx.t_args(
											"plugin-by",
											&[("author", ArgValue::from(author.to_string()))],
										),
										theme,
									)),
							)
							.child(
								Switch::new(("plugin-enabled", index))
									.checked(info.enabled && !incompatible)
									.disabled(safe_mode || incompatible)
									.on_change(cx.listener(move |this, checked: &bool, _, cx| {
										this.set_enabled(&toggle_id, *checked, cx)
									})),
							),
					)
					.when(!info.description.is_empty(), |card| {
						card.child(
							div()
								.text_size(theme.font_sm)
								.text_color(theme.text_normal)
								.child(info.description.clone()),
						)
					})
					.children(info.error.clone().map(|error| {
						div()
							.text_size(theme.font_sm)
							.text_color(theme.danger)
							.child(error)
					}))
					.child(
						div()
							.flex()
							.flex_wrap()
							.items_center()
							.gap(theme.space_8)
							.child(
								Button::new(("plugin-reload", index))
									.small()
									.label(cx.t("plugin-reload"))
									.disabled(!info.enabled || incompatible)
									.on_click(cx.listener(move |this, _, _, cx| {
										this.send(Command::PluginReload(reload_id.clone()), cx)
									})),
							)
							.child(
								Button::new(("plugin-settings", index))
									.small()
									.label(cx.t("plugin-settings"))
									.disabled(incompatible)
									.on_click(cx.listener(move |this, _, window, cx| {
										this.open_form(form_id.clone(), form_name.clone(), window, cx)
									})),
							)
							.when(info.needs_prompt, |row| {
								row.child(
									Button::new(("plugin-review", index))
										.small()
										.primary()
										.label(cx.t("plugin-review"))
										.on_click(cx.listener(move |this, _, window, cx| {
											this.review_installed(&review, window, cx)
										})),
								)
							})
							.child(
								Button::new(("plugin-uninstall", index))
									.small()
									.danger()
									.label(cx.t("plugin-uninstall"))
									.on_click(cx.listener(move |this, _, window, cx| {
										this.confirm_uninstall(
											remove_id.clone(),
											remove_name.clone(),
											window,
											cx,
										)
									})),
							),
					),
			)
			.into_any_element()
	}

	fn installed(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let safe_mode = cx.try_global::<AppFlags>().is_some_and(|flags| flags.safe_mode);
		let list = self.data.read(cx).session().plugin_list();
		let needs_review = list.iter().any(|info| info.needs_prompt);
		let error = self.data.read(cx).session().plugin_error();
		let cards: Vec<AnyElement> = list
			.iter()
			.enumerate()
			.map(|(index, info)| self.plugin_card(index, info, safe_mode, theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.when(safe_mode, |page| {
				page.child(banner(cx.t("plugin-safe-banner"), theme.warning, theme))
			})
			.when(needs_review, |page| {
				page.child(banner(cx.t("plugin-review-banner"), theme.brand, theme))
			})
			.children(error.map(|error| banner(error, theme.danger, theme)))
			.children(
				self.notice
					.clone()
					.map(|notice| banner(notice, theme.danger, theme)),
			)
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.gap(theme.space_16)
					.child(
						Button::new("plugin-install-file")
							.primary()
							.label(cx.t("plugin-install-file"))
							.on_click(cx.listener(|this, _, window, cx| this.pick_file(window, cx))),
					)
					.child(
						div()
							.flex()
							.flex_col()
							.items_end()
							.gap(theme.space_2)
							.child(
								Switch::new("plugin-safe-mode")
									.checked(self.safe_mode_setting)
									.label(cx.t("plugin-safe-mode"))
									.on_change(cx.listener(|this, checked: &bool, _, cx| {
										this.set_safe_mode(*checked, cx)
									})),
							)
							.child(note(cx.t("plugin-safe-mode-note"), theme)),
					),
			)
			.when(cards.is_empty(), |page| {
				page.child(note(cx.t("plugin-empty"), theme))
			})
			.children(cards)
			.into_any_element()
	}

	fn catalog_entry(
		&self,
		index: usize,
		entry: &CatalogEntry,
		installed: &[PluginInfo],
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let present = installed
			.iter()
			.any(|info| info.id == entry.id && info.version == entry.version);
		let target = entry.clone();
		card(theme)
			.child(
				div()
					.flex()
					.items_start()
					.gap(theme.space_12)
					.child(icon_tile(IconKind::Plugins, theme))
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
											.text_size(theme.font_md)
											.font_weight(FontWeight::SEMIBOLD)
											.text_color(theme.header_primary)
											.child(entry.name.clone()),
									)
									.child(
										div()
											.text_size(theme.font_sm)
											.text_color(theme.text_muted)
											.child(format!("v{}", entry.version)),
									)
									.when(entry.reviewed, |row| {
										row.child(chip(cx.t("catalog-reviewed"), theme.success, theme))
									}),
							)
							.child(note(
								cx.t_args(
									"catalog-permissions",
									&[("count", ArgValue::from(entry.permissions.len()))],
								),
								theme,
							)),
					)
					.child(
						Button::new(("catalog-install", index))
							.small()
							.primary()
							.label(if present {
								cx.t("catalog-installed")
							} else {
								cx.t("catalog-install")
							})
							.disabled(present)
							.on_click(cx.listener(move |this, _, window, cx| {
								this.review_catalog(&target, window, cx)
							})),
					),
			)
			.when(!entry.description.is_empty(), |card| {
				card.child(
					div()
						.text_size(theme.font_sm)
						.text_color(theme.text_normal)
						.child(entry.description.clone()),
				)
			})
			.into_any_element()
	}

	fn catalog(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let session = self.data.read(cx).session();
		let catalog: Option<Arc<Catalog>> = session.catalog();
		let error = session.catalog_error();
		let installed = session.plugin_list();
		let entries: Vec<AnyElement> = catalog
			.iter()
			.flat_map(|catalog| catalog.plugins.iter())
			.enumerate()
			.map(|(index, entry)| self.catalog_entry(index, entry, &installed, theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(section_label("catalog-url", theme, cx))
					.child(
						div()
							.flex()
							.items_center()
							.gap(theme.space_8)
							.child(
								div()
									.flex()
									.items_center()
									.flex_1()
									.min_w_0()
									.h(rem(URL_HEIGHT))
									.px(theme.space_8)
									.rounded(theme.radius_sm)
									.bg(theme.input_background)
									.child(
										Input::new(&self.catalog_url)
											.appearance(false)
											.bordered(false)
											.focus_bordered(false)
											.small()
											.w_full(),
									),
							)
							.child(
								Button::new("catalog-refresh")
									.primary()
									.label(cx.t("catalog-refresh"))
									.on_click(
										cx.listener(|this, _, window, cx| this.confirm_refresh(window, cx)),
									),
							),
					),
			)
			.children(error.map(|error| banner(error, theme.danger, theme)))
			.when(entries.is_empty() && catalog.is_none(), |page| {
				page.child(note(cx.t("catalog-empty"), theme))
			})
			.children(entries)
			.into_any_element()
	}

	fn tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let weak = cx.weak_entity();
		let pick = |tab: Tab, id: &'static str, key: &str| {
			let weak = weak.clone();
			tab_pill(id, cx.t(key), self.tab == tab, theme, move |_, cx| {
				weak.update(cx, |this, cx| {
					this.tab = tab;
					cx.notify();
				})
				.ok();
			})
		};
		div()
			.flex()
			.items_center()
			.gap(theme.space_4)
			.child(pick(
				Tab::Installed,
				"plugin-tab-installed",
				"plugin-tab-installed",
			))
			.child(pick(Tab::Catalog, "plugin-tab-catalog", "plugin-tab-catalog"))
			.into_any_element()
	}

	fn detail(&self, form: &Entity<PluginForm>, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let name = form.read(cx).name().clone();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_12)
					.child(
						Button::new("plugin-back")
							.small()
							.label(cx.t("plugin-back"))
							.on_click(cx.listener(|this, _, _, cx| {
								this.form = None;
								cx.notify();
							})),
					)
					.child(heading(name, theme)),
			)
			.child(form.clone())
			.into_any_element()
	}
}

impl Render for PluginsPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		if let Some(form) = self.form.clone() {
			return self.detail(&form, &theme, cx);
		}
		let body = match self.tab {
			Tab::Installed => self.installed(&theme, cx),
			Tab::Catalog => self.catalog(&theme, cx),
		};
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(self.tabs(&theme, cx))
			.child(body)
			.into_any_element()
	}
}
