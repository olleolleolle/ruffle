mod controller;
mod movie;
mod open_dialog;

pub use controller::GuiController;
pub use movie::MovieView;
use std::borrow::Cow;

use crate::custom_event::RuffleEvent;
use crate::gui::open_dialog::OpenDialog;
use crate::player::PlayerOptions;
use crate::util::pick_file;
use chrono::DateTime;
use egui::*;
use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{static_loader, Loader};
use ruffle_core::backend::ui::US_ENGLISH;
use ruffle_core::Player;
use std::collections::HashMap;
use sys_locale::get_locale;
use unic_langid::LanguageIdentifier;
use url::Url;
use winit::event_loop::EventLoopProxy;

const VERGEN_UNKNOWN: &str = "VERGEN_IDEMPOTENT_OUTPUT";

static_loader! {
    static TEXTS = {
        locales: "./assets/texts",
        fallback_language: "en-US"
    };
}

pub fn text<'a>(locale: &LanguageIdentifier, id: &'a str) -> Cow<'a, str> {
    TEXTS.lookup(locale, id).map(Cow::Owned).unwrap_or_else(|| {
        tracing::error!("Unknown desktop text id '{id}'");
        Cow::Borrowed(id)
    })
}

#[allow(dead_code)]
pub fn text_with_args<'a, T: AsRef<str>>(
    locale: &LanguageIdentifier,
    id: &'a str,
    args: &HashMap<T, FluentValue>,
) -> Cow<'a, str> {
    TEXTS
        .lookup_with_args(locale, id, args)
        .map(Cow::Owned)
        .unwrap_or_else(|| {
            tracing::error!("Unknown desktop text id '{id}'");
            Cow::Borrowed(id)
        })
}

/// Size of the top menu bar in pixels.
/// This is the offset at which the movie will be shown,
/// and added to the window size if trying to match a movie.
pub const MENU_HEIGHT: u32 = 24;

/// The main controller for the Ruffle GUI.
pub struct RuffleGui {
    event_loop: EventLoopProxy<RuffleEvent>,
    is_about_visible: bool,
    context_menu: Vec<ruffle_core::ContextMenuItem>,
    locale: LanguageIdentifier,
    default_player_options: PlayerOptions,
    open_dialog: Option<OpenDialog>,
}

impl RuffleGui {
    fn new(event_loop: EventLoopProxy<RuffleEvent>, default_player_options: PlayerOptions) -> Self {
        // TODO: language negotiation + https://github.com/1Password/sys-locale/issues/14
        // This should also be somewhere else so it can be supplied through UiBackend too

        let preferred_locale = get_locale();
        let locale = preferred_locale
            .and_then(|l| l.parse().ok())
            .unwrap_or_else(|| US_ENGLISH.clone());

        Self {
            event_loop,
            is_about_visible: false,
            context_menu: vec![],
            locale,
            default_player_options,
            open_dialog: None,
        }
    }

    /// Renders all of the main Ruffle UI, including the main menu and context menus.
    fn update(
        &mut self,
        egui_ctx: &egui::Context,
        show_menu: bool,
        has_movie: bool,
        player: &mut Option<&mut Player>,
    ) {
        if show_menu {
            self.main_menu_bar(egui_ctx, has_movie, player);
        }

        self.about_window(egui_ctx);
        self.open_dialog(egui_ctx);

        if !self.context_menu.is_empty() {
            self.context_menu(egui_ctx);
        }
    }

    pub fn show_context_menu(&mut self, menu: Vec<ruffle_core::ContextMenuItem>) {
        self.context_menu = menu;
    }

    pub fn is_context_menu_visible(&self) -> bool {
        !self.context_menu.is_empty()
    }

    /// Renders the main menu bar at the top of the window.
    fn main_menu_bar(
        &mut self,
        egui_ctx: &egui::Context,
        has_movie: bool,
        player: &mut Option<&mut Player>,
    ) {
        egui::TopBottomPanel::top("menu_bar").show(egui_ctx, |ui| {
            // TODO(mike): Make some MenuItem struct with shortcut info to handle this more cleanly.
            if ui.ctx().input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O))
            }) {
                self.open_file(ui);
            }
            if ui.ctx().input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Q))
            }) {
                self.request_exit(ui);
            }
            if ui.ctx().input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::P))
            }) {
                if let Some(player) = player {
                    player.set_is_playing(!player.is_playing());
                }
            }

            menu::bar(ui, |ui| {
                menu::menu_button(ui, text(&self.locale, "file-menu"), |ui| {
                    let mut shortcut;
                    shortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);

                    if Button::new(text(&self.locale, "file-menu-open-quick"))
                        .shortcut_text(ui.ctx().format_shortcut(&shortcut))
                        .ui(ui)
                        .clicked()
                    {
                        self.open_file(ui);
                    }

                    if Button::new(text(&self.locale, "file-menu-open-advanced")).ui(ui).clicked() {
                        self.open_file_advanced(ui);
                    }

                    if ui.add_enabled(has_movie, Button::new(text(&self.locale, "file-menu-close"))).clicked() {
                        self.close_movie(ui);
                    }

                    ui.separator();

                    shortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
                    if Button::new(text(&self.locale, "file-menu-exit"))
                        .shortcut_text(ui.ctx().format_shortcut(&shortcut))
                        .ui(ui)
                        .clicked()
                    {
                        self.request_exit(ui);
                    }
                });
                menu::menu_button(ui, text(&self.locale, "controls-menu"), |ui| {
                    ui.add_enabled_ui(player.is_some(), |ui| {
                        let playing = player.as_ref().map(|p| p.is_playing()).unwrap_or_default();
                        let pause_shortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
                        if Button::new(text(&self.locale, if playing { "controls-menu-suspend" } else { "controls-menu-resume" })).shortcut_text(ui.ctx().format_shortcut(&pause_shortcut)).ui(ui).clicked() {
                            ui.close_menu();
                            if let Some(player) = player {
                                player.set_is_playing(!player.is_playing());
                            }
                        }
                    });
                });
                menu::menu_button(ui, text(&self.locale, "help-menu"), |ui| {
                    if ui.button(text(&self.locale, "help-menu-join-discord")).clicked() {
                        self.launch_website(ui, "https://discord.gg/ruffle");
                    }
                    if ui.button(text(&self.locale, "help-menu-report-a-bug")).clicked() {
                        self.launch_website(ui, "https://github.com/ruffle-rs/ruffle/issues/new?assignees=&labels=bug&projects=&template=bug_report.yml");
                    }
                    if ui.button(text(&self.locale, "help-menu-sponsor-development")).clicked() {
                        self.launch_website(ui, "https://opencollective.com/ruffle/");
                    }
                    if ui.button(text(&self.locale, "help-menu-translate-ruffle")).clicked() {
                        self.launch_website(ui, "https://crowdin.com/project/ruffle");
                    }
                    ui.separator();
                    if ui.button(text(&self.locale, "help-menu-about")).clicked() {
                        self.show_about_screen(ui);
                    }
                });
            });
        });
    }

    fn about_window(&mut self, egui_ctx: &egui::Context) {
        egui::Window::new(text(&self.locale, "about-ruffle"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut self.is_about_visible)
            .show(egui_ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Ruffle")
                            .color(Color32::from_rgb(0xFF, 0xAD, 0x33))
                            .size(32.0),
                    );
                    Grid::new("about_ruffle_version_info")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(text(&self.locale, "about-ruffle-version"));
                            ui.label(env!("CARGO_PKG_VERSION"));
                            ui.end_row();

                            ui.label(text(&self.locale, "about-ruffle-channel"));
                            ui.label(env!("CFG_RELEASE_CHANNEL"));
                            ui.end_row();

                            let build_time = env!("VERGEN_BUILD_TIMESTAMP");
                            if build_time != VERGEN_UNKNOWN {
                                ui.label(text(&self.locale, "about-ruffle-build-time"));
                                ui.label(
                                    DateTime::parse_from_rfc3339(build_time)
                                        .map(|t| t.format("%c").to_string())
                                        .unwrap_or_else(|_| build_time.to_string()),
                                );
                                ui.end_row();
                            }

                            let sha = env!("VERGEN_GIT_SHA");
                            if sha != VERGEN_UNKNOWN {
                                ui.label(text(&self.locale, "about-ruffle-commit-ref"));
                                ui.hyperlink_to(
                                    sha,
                                    format!("https://github.com/ruffle-rs/ruffle/commit/{}", sha),
                                );
                                ui.end_row();
                            }

                            let commit_time = env!("VERGEN_GIT_COMMIT_TIMESTAMP");
                            if sha != VERGEN_UNKNOWN {
                                ui.label(text(&self.locale, "about-ruffle-commit-time"));
                                ui.label(
                                    DateTime::parse_from_rfc3339(commit_time)
                                        .map(|t| t.format("%c").to_string())
                                        .unwrap_or_else(|_| commit_time.to_string()),
                                );
                                ui.end_row();
                            }

                            ui.label(text(&self.locale, "about-ruffle-build-features"));
                            ui.horizontal_wrapped(|ui| {
                                ui.label(env!("VERGEN_CARGO_FEATURES").replace(',', ", "));
                            });
                            ui.end_row();
                        });

                    ui.horizontal(|ui| {
                        ui.hyperlink_to(
                            text(&self.locale, "about-ruffle-visit-website"),
                            "https://ruffle.rs",
                        );
                        ui.hyperlink_to(
                            text(&self.locale, "about-ruffle-visit-github"),
                            "https://github.com/ruffle-rs/ruffle/",
                        );
                        ui.hyperlink_to(
                            text(&self.locale, "about-ruffle-visit-discord"),
                            "https://discord.gg/ruffle",
                        );
                        ui.hyperlink_to(
                            text(&self.locale, "about-ruffle-visit-sponsor"),
                            "https://opencollective.com/ruffle/",
                        );
                        ui.shrink_width_to_current();
                    });
                })
            });
    }

    /// Renders the right-click context menu.
    fn context_menu(&mut self, egui_ctx: &egui::Context) {
        let mut item_clicked = false;
        let mut menu_visible = false;
        // TODO: What is the proper way in egui to spawn a random context menu?
        egui::CentralPanel::default()
            .frame(Frame::none())
            .show(egui_ctx, |_| {})
            .response
            .context_menu(|ui| {
                menu_visible = true;
                for (i, item) in self.context_menu.iter().enumerate() {
                    if i != 0 && item.separator_before {
                        ui.separator();
                    }
                    let clicked = if item.checked {
                        Checkbox::new(&mut true, &item.caption).ui(ui).clicked()
                    } else {
                        Button::new(&item.caption).ui(ui).clicked()
                    };
                    if clicked {
                        let _ = self
                            .event_loop
                            .send_event(RuffleEvent::ContextMenuItemClicked(i));
                        item_clicked = true;
                    }
                }
            });

        if item_clicked
            || !menu_visible
            || egui_ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            // Hide menu.
            self.context_menu.clear();
        }
    }

    fn open_file(&mut self, ui: &mut egui::Ui) {
        ui.close_menu();

        if let Some(url) = pick_file().and_then(|p| Url::from_file_path(p).ok()) {
            let _ = self.event_loop.send_event(RuffleEvent::OpenURL(
                url,
                Box::new(self.default_player_options.clone()),
            ));
        }
    }

    fn open_file_advanced(&mut self, ui: &mut egui::Ui) {
        ui.close_menu();

        self.open_dialog = Some(OpenDialog::new(
            self.default_player_options.clone(),
            self.event_loop.clone(),
            self.locale.clone(),
        ));
    }

    fn close_movie(&mut self, ui: &mut egui::Ui) {
        let _ = self.event_loop.send_event(RuffleEvent::CloseFile);
        ui.close_menu();
    }

    fn open_dialog(&mut self, egui_ctx: &egui::Context) {
        let keep_open = self
            .open_dialog
            .as_mut()
            .map(|d| d.show(egui_ctx))
            .unwrap_or_default();
        if !keep_open {
            self.open_dialog = None;
        }
    }

    fn request_exit(&mut self, ui: &mut egui::Ui) {
        let _ = self.event_loop.send_event(RuffleEvent::ExitRequested);
        ui.close_menu();
    }

    fn launch_website(&mut self, ui: &mut egui::Ui, url: &str) {
        let _ = webbrowser::open(url);
        ui.close_menu();
    }

    fn show_about_screen(&mut self, ui: &mut egui::Ui) {
        self.is_about_visible = true;
        ui.close_menu();
    }
}