//! The desktop shell: capture, decisions, search and staleness in one window.
//!
//! The shell is a view over the same library the CLI uses. It holds no state
//! the vault does not have, so closing the window loses nothing and a crash
//! cannot corrupt a note.

use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::classify::Classification;
use crate::error::Result;
use crate::ledger::DecisionCard;
use crate::search::SearchHit;
use crate::stale::StaleFlag;
use crate::stats::Stats;
use crate::Vault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Capture,
    Decisions,
    Search,
    Stale,
    Status,
}

impl View {
    const ALL: [View; 5] = [
        View::Capture,
        View::Decisions,
        View::Search,
        View::Stale,
        View::Status,
    ];

    fn label(self) -> &'static str {
        match self {
            View::Capture => "Capture",
            View::Decisions => "Decisions",
            View::Search => "Search",
            View::Stale => "Stale",
            View::Status => "Status",
        }
    }
}

/// Fonts bundled with the shell: Latin from egui, ten Indic scripts from the
/// Lohit project (SIL OFL 1.1, see `assets/fonts/OFL.txt`).
pub const BUNDLED_FONTS: [(&str, &[u8]); 10] = [
    (
        "lohit-devanagari",
        include_bytes!("../../assets/fonts/Lohit-Devanagari.ttf"),
    ),
    (
        "lohit-gujarati",
        include_bytes!("../../assets/fonts/Lohit-Gujarati.ttf"),
    ),
    (
        "lohit-bengali",
        include_bytes!("../../assets/fonts/Lohit-Bengali.ttf"),
    ),
    (
        "lohit-assamese",
        include_bytes!("../../assets/fonts/Lohit-Assamese.ttf"),
    ),
    (
        "lohit-gurmukhi",
        include_bytes!("../../assets/fonts/Lohit-Gurmukhi.ttf"),
    ),
    (
        "lohit-odia",
        include_bytes!("../../assets/fonts/Lohit-Odia.ttf"),
    ),
    (
        "lohit-tamil",
        include_bytes!("../../assets/fonts/Lohit-Tamil.ttf"),
    ),
    (
        "lohit-telugu",
        include_bytes!("../../assets/fonts/Lohit-Telugu.ttf"),
    ),
    (
        "lohit-kannada",
        include_bytes!("../../assets/fonts/Lohit-Kannada.ttf"),
    ),
    (
        "lohit-malayalam",
        include_bytes!("../../assets/fonts/Lohit-Malayalam.ttf"),
    ),
];

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in BUNDLED_FONTS {
        fonts
            .font_data
            .insert(name.to_string(), egui::FontData::from_static(bytes));
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let entry = fonts.families.entry(family).or_default();
        for (name, _) in BUNDLED_FONTS {
            entry.push(name.to_string());
        }
    }
    ctx.set_fonts(fonts);
}

/// The two capture shortcuts: one to write, one to look something up.
pub fn hotkeys() -> [(HotKey, View); 2] {
    [
        (
            HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space),
            View::Capture,
        ),
        (
            HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyK),
            View::Search,
        ),
    ]
}

pub struct App {
    hotkey_manager: Option<GlobalHotKeyManager>,
    vault: Vault,
    view: View,
    capture: String,
    preview: Option<Classification>,
    query: String,
    decisions: Vec<DecisionCard>,
    hits: Vec<SearchHit>,
    flags: Vec<StaleFlag>,
    stats: Option<Stats>,
    status: String,
    offer_wrap: bool,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("view", &self.view)
            .field("vault", &self.vault.root())
            .finish()
    }
}

impl App {
    pub fn new(vault: Vault) -> App {
        let mut app = App {
            hotkey_manager: None,
            vault,
            view: View::Capture,
            capture: String::new(),
            preview: None,
            query: String::new(),
            decisions: Vec::new(),
            hits: Vec::new(),
            flags: Vec::new(),
            stats: None,
            status: String::new(),
            offer_wrap: false,
        };
        app.refresh();
        let date = crate::vault::today();
        app.offer_wrap = crate::daily::wrap_due(
            chrono::Local::now(),
            crate::daily::wrap_written(&app.vault, &date),
        );
        app
    }

    /// Register the global shortcuts. A machine without a hotkey service simply
    /// runs without them.
    fn register_hotkeys(&mut self) -> bool {
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(_) => return false,
        };
        let mut registered = false;
        for (hotkey, _) in hotkeys() {
            if manager.register(hotkey).is_ok() {
                registered = true;
            }
        }
        if registered {
            self.hotkey_manager = Some(manager);
        }
        registered
    }

    fn drain_hotkeys(&mut self, ctx: &egui::Context) {
        if self.hotkey_manager.is_none() {
            return;
        }
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state != HotKeyState::Pressed {
                continue;
            }
            for (hotkey, view) in hotkeys() {
                if hotkey.id() == event.id {
                    self.view = view;
                    self.refresh();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
    }

    fn refresh(&mut self) {
        match self.view {
            View::Decisions => match crate::ledger::cards(&self.vault) {
                Ok(cards) => self.decisions = cards,
                Err(err) => self.status = format!("could not read the ledger: {err}"),
            },
            View::Search => {
                let filters = crate::search::Filters::default();
                match crate::search::search(&self.vault, &self.query, &filters) {
                    Ok(hits) => self.hits = hits,
                    Err(err) => self.status = format!("search failed: {err}"),
                }
            }
            View::Stale => match crate::stale::scan(&self.vault) {
                Ok(flags) => self.flags = flags,
                Err(err) => self.status = format!("stale scan failed: {err}"),
            },
            View::Status => match crate::stats::compute(&self.vault) {
                Ok(stats) => self.stats = Some(stats),
                Err(err) => self.status = format!("stats failed: {err}"),
            },
            View::Capture => {}
        }
    }

    fn classify_now(&mut self) {
        if self.capture.trim().is_empty() {
            self.preview = None;
        } else {
            self.preview = Some(crate::capture::preview(&self.capture));
        }
    }

    fn save_capture(&mut self) {
        if self.capture.trim().is_empty() {
            return;
        }
        match crate::capture::capture(&self.vault, &self.capture) {
            Ok(outcome) => {
                self.status = outcome.describe();
                self.capture.clear();
                self.preview = None;
            }
            Err(err) => self.status = format!("could not save: {err}"),
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("DayDoc");
        ui.label(
            egui::RichText::new(self.vault.root().display().to_string())
                .small()
                .weak(),
        );
        ui.separator();
        for view in View::ALL {
            if ui.selectable_label(self.view == view, view.label()).clicked() {
                self.view = view;
                self.refresh();
            }
        }
        ui.separator();
        if ui.button("Refresh").clicked() {
            self.refresh();
        }
        if ui.button("Write handover").clicked() {
            let to = crate::vault::today();
            let from = (chrono::Local::now().date_naive() - chrono::Duration::days(14))
                .format("%Y-%m-%d")
                .to_string();
            match crate::handover::build(&self.vault, &from, &to)
                .and_then(|packet| crate::handover::write(&self.vault, &packet))
            {
                Ok(rel) => self.status = format!("handover written to {rel}"),
                Err(err) => self.status = format!("handover failed: {err}"),
            }
        }
        if ui.button("Backup now").clicked() {
            let dir = crate::backup::default_dir(&self.vault);
            match crate::backup::run(&self.vault, &dir) {
                Ok(report) => self.status = report.render().trim().to_string(),
                Err(err) => self.status = format!("backup failed: {err}"),
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("LLM: off unless configured").small().weak());
    }

    fn wrap_banner(&mut self, ui: &mut egui::Ui) {
        if !self.offer_wrap {
            return;
        }
        let mut dismiss = false;
        let mut write = false;
        ui.group(|ui| {
            ui.label(
                egui::RichText::new("It is after 17:00. Write the end-of-day wrap?")
                    .strong(),
            );
            ui.horizontal(|ui| {
                if ui.button("Write wrap").clicked() {
                    write = true;
                }
                if ui.button("Not now").clicked() {
                    dismiss = true;
                }
            });
        });
        if write {
            let date = crate::vault::today();
            match crate::daily::write_wrap(&self.vault, &date) {
                Ok(rel) => self.status = format!("wrap written into {rel}"),
                Err(err) => self.status = format!("wrap failed: {err}"),
            }
            self.offer_wrap = false;
        }
        if dismiss {
            self.offer_wrap = false;
        }
    }

    fn capture_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Capture");
        self.wrap_banner(ui);
        ui.label("One box. Type, then Ctrl+Enter.");
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.capture)
                .desired_rows(6)
                .desired_width(f32::INFINITY)
                .hint_text("we decided to ... / - [ ] ... / notes from the call ..."),
        );
        if response.changed() {
            self.classify_now();
        }

        if let Some(preview) = &self.preview {
            let confident = preview.is_confident(self.vault.config().min_classification_confidence);
            let colour = if confident {
                egui::Color32::from_rgb(60, 130, 90)
            } else {
                egui::Color32::from_rgb(150, 120, 60)
            };
            ui.label(
                egui::RichText::new(format!(
                    "→ {} at {:.0}%{}",
                    preview.note_type,
                    preview.confidence * 100.0,
                    if confident {
                        ""
                    } else {
                        " — will land in inbox"
                    }
                ))
                .color(colour),
            );
        }

        let pressed_ctrl_enter = ui.input(|input| {
            input.modifiers.command && input.key_pressed(egui::Key::Enter)
        });
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() || pressed_ctrl_enter {
                self.save_capture();
            }
            if ui.button("Clear").clicked() {
                self.capture.clear();
                self.preview = None;
            }
            ui.label(egui::RichText::new("Ctrl+Enter saves").small().weak());
        });
    }

    fn decisions_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Decisions");
        if self.decisions.is_empty() {
            ui.label("No decisions yet. Capture one, or paste a meeting.");
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for card in &self.decisions {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&card.decision_id).strong().monospace(),
                            );
                            ui.label(egui::RichText::new(card.status.as_str()).weak());
                        });
                        ui.label(egui::RichText::new(&card.title).heading());
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {} · supersedes {} · cited by {}",
                                card.decided_at.as_deref().unwrap_or("undated"),
                                card.owner.as_deref().unwrap_or("no owner"),
                                card.supersedes.as_deref().unwrap_or("none"),
                                card.cited_by
                            ))
                            .small()
                            .weak(),
                        );
                    });
                }
            });
    }

    fn search_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Search");
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(320.0)
                    .hint_text("what did we decide about ..."),
            );
            if response.changed() || ui.button("Search").clicked() {
                self.refresh();
            }
        });
        if self.hits.is_empty() {
            ui.label(egui::RichText::new("no matches").weak());
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for hit in &self.hits {
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(&hit.title).strong());
                        ui.label(
                            egui::RichText::new(format!("{} · {}", hit.note_type, hit.rel_path))
                                .small()
                                .weak(),
                        );
                        ui.label(egui::RichText::new(&hit.excerpt).small());
                    });
                }
            });
    }

    fn stale_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Stale");
        if self.flags.is_empty() {
            ui.label("Nothing is stale.");
            return;
        }
        // Clicks are collected first: the list borrows `self.flags`, and
        // re-verifying writes to the vault.
        let mut to_verify: Option<String> = None;
        let flags = &self.flags;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for flag in flags {
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(&flag.title).strong());
                        ui.label(
                            egui::RichText::new(format!("{} · {}", flag.note_type, flag.rel_path))
                                .small()
                                .weak(),
                        );
                        for reason in &flag.reasons {
                            ui.label(format!("· {}", reason.describe()));
                        }
                        if ui.button("Re-verify").clicked() {
                            to_verify = Some(flag.rel_path.clone());
                        }
                    });
                }
            });
        if let Some(rel) = to_verify {
            match crate::stale::reverify(&self.vault, &rel) {
                Ok(note) => {
                    self.status = format!("re-verified {}", note.rel_path);
                    self.refresh();
                }
                Err(err) => self.status = format!("failed: {err}"),
            }
        }
    }

    fn status_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Status");
        match &self.stats {
            Some(stats) => {
                ui.label(egui::RichText::new(stats.render()).monospace());
            }
            None => {
                ui.label("No statistics yet.");
            }
        }
    }
}

impl App {
    /// One frame. Kept separate from the `eframe::App` impl so a frame can be
    /// rendered without a window in a test.
    pub fn draw(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("daydoc-side")
            .resizable(false)
            .default_width(190.0)
            .show(ctx, |ui| self.side_panel(ui));

        egui::TopBottomPanel::bottom("daydoc-status").show(ctx, |ui| {
            if self.status.is_empty() {
                ui.label(egui::RichText::new("ready").small().weak());
            } else {
                ui.label(egui::RichText::new(&self.status).small());
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Capture => self.capture_view(ui),
            View::Decisions => self.decisions_view(ui),
            View::Search => self.search_view(ui),
            View::Stale => self.stale_view(ui),
            View::Status => self.status_view(ui),
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_hotkeys(ctx);
        self.draw(ctx);
    }
}

/// Open the window. Blocks until the user closes it.
pub fn run(vault: Vault) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title("DayDoc"),
        ..Default::default()
    };
    let creator = Box::new(move |cc: &eframe::CreationContext<'_>| {
        install_fonts(&cc.egui_ctx);
        let mut app = App::new(vault);
        app.register_hotkeys();
        Ok(Box::new(app) as Box<dyn eframe::App>)
    });
    eframe::run_native("DayDoc", options, creator)
        .map_err(|err| crate::error::DayDocError::Config(format!("could not open a window: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn the_app_starts_on_capture_and_refreshes_cleanly() {
        let vault = temp_vault();
        let mut app = App::new(vault);
        assert_eq!(app.view, View::Capture);
        app.view = View::Decisions;
        app.refresh();
        assert!(app.decisions.is_empty());
        app.view = View::Status;
        app.refresh();
        assert!(app.stats.is_some());
    }

    #[test]
    fn a_capture_saved_from_the_shell_becomes_a_note() {
        let vault = temp_vault();
        let mut app = App::new(vault);
        app.capture = "We decided to adopt Okta for SSO across all products.".to_string();
        app.classify_now();
        assert!(app.preview.is_some());
        app.save_capture();
        assert!(app.capture.is_empty());
        assert!(app.status.contains("DR-0001"), "{}", app.status);
        assert!(app.vault.exists("decisions/DR-0001.md"));
    }

    #[test]
    fn the_shortcuts_are_ctrl_shift_space_and_ctrl_shift_k() {
        let [(space, space_view), (key_k, k_view)] = hotkeys();
        assert_eq!(space.id(), HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space).id());
        assert_eq!(key_k.id(), HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyK).id());
        assert_eq!(space_view, View::Capture);
        assert_eq!(k_view, View::Search);
    }

    #[test]
    fn every_indic_font_is_bundled_and_is_a_truetype_file() {
        assert_eq!(BUNDLED_FONTS.len(), 10);
        for (name, bytes) in BUNDLED_FONTS {
            assert!(bytes.len() > 10_000, "{name} looks empty");
            // TrueType signature: 0x00010000.
            assert_eq!(&bytes[..4], &[0x00, 0x01, 0x00, 0x00], "{name} is not a TTF");
        }
    }

    fn render_frames(app: &mut App, frames: usize) -> usize {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut shapes = 0usize;
        for _ in 0..frames {
            let output = ctx.run(egui::RawInput::default(), |ctx| app.draw(ctx));
            shapes = output.shapes.len();
        }
        shapes
    }

    #[test]
    fn every_view_renders_a_frame_without_a_window() {
        let vault = temp_vault();
        crate::ledger::create(
            &vault,
            &crate::ledger::NewDecision::new("Use Okta for SSO")
                .accepted()
                .with_owner("alice"),
        )
        .expect("decision");
        vault
            .write(
                "reference/old.md",
                "---\nid: 01A\ntype: reference\ntitle: Old\nlast_verified: 2020-01-01\n---\nbody\n",
            )
            .expect("write");

        let mut app = App::new(vault);
        for view in View::ALL {
            app.view = view;
            app.refresh();
            let shapes = render_frames(&mut app, 2);
            assert!(shapes > 0, "{} drew nothing", view.label());
        }
    }

    #[test]
    fn the_capture_view_renders_with_text_and_a_preview() {
        let vault = temp_vault();
        let mut app = App::new(vault);
        app.capture = "we decided to adopt okta".to_string();
        app.classify_now();
        let shapes = render_frames(&mut app, 2);
        assert!(shapes > 0);
        assert!(app.preview.is_some());
    }

    #[test]
    fn a_view_label_exists_for_every_view() {
        for view in View::ALL {
            assert!(!view.label().is_empty());
        }
    }
}
