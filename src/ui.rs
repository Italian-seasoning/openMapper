//! The interface, laid out like a desktop editor: a menu bar, a toolbar, the tool's panel
//! on the left (catalogue, brushes, layers), the properties of what is selected on the
//! right, a status bar at the bottom, and the 3D view in the middle. Dialogs are plain
//! windows. Help for whatever the mouse is over goes to the status bar.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Align, Color32, CornerRadius, Frame, Id, Layout, Margin, RichText, Sense, Stroke, Vec2};

use crate::doc::Sel;
use crate::editor::{Editor, LogicMode, TerrainMode, Tool};
use crate::i18n::{self, tr, Lang};
use crate::icons::{self, Icon};
use crate::install::{MapEntry, Prefs};
use crate::library::{Category, Kind, Library};
use crate::render::Thumb;

pub const ACCENT: Color32 = Color32::from_rgb(232, 122, 50);
const BG: Color32 = Color32::from_rgb(36, 37, 40);
const BG_DARK: Color32 = Color32::from_rgb(27, 28, 30);
const BG_FIELD: Color32 = Color32::from_rgb(22, 23, 25);
const BORDER: Color32 = Color32::from_rgb(18, 18, 20);
const HEADER: Color32 = Color32::from_rgb(45, 46, 50);
pub(crate) const TEXT_DIM: Color32 = Color32::from_rgb(140, 143, 150);
const SELECT: Color32 = Color32::from_rgb(64, 86, 122);

/// What the interface asks the application to do.
pub enum Action {
    NewMap { folder: String, friendly: String, size: i32, options: crate::ui_newmap::NewMapOptions },
    Open(PathBuf),
    OpenDialog,
    Save,
    Close,
    Play,
    Undo,
    Redo,
    Import,
    ChooseOmsi,
    SetLang(Lang),
    /// The interface's zoom (1 = as the system has it).
    SetScale(f32),
    FocusSelection,
    /// Star a catalogue entry or take its star away.
    ToggleFavourite(String),
    /// A catalogue entry was chosen (for the recently used list).
    UsedAsset(String),
    /// The 3D view becomes the map's picture (`picture.jpg`).
    MapPicture,
    /// The map opens with the camera as it is now (`[mapcam]`).
    SetMapCamera,
    Copy,
    Cut,
    Paste,
    Duplicate,
    SelectAll,
    /// Build the road drawn so far.
    BuildRoad,
    /// Move the camera over this point (the overview map).
    LookAt(glam::DVec2),
    AddGroundTexture(String),
    /// Open or close the Lines window.
    Timetable,
    /// Put the map's folder back as it was before the save kept in this backup (and the
    /// later ones), then open it again.
    RestoreBackup(PathBuf),
    /// Open or close the Traffic and AI window.
    Traffic,
    /// View > Graphics quality.
    SetGraphics(crate::render::quality::Quality),
    /// Open or close the Getting started guide.
    Guide,
    /// Save the map and pack it into a .zip to share.
    Export,
    /// Do it without asking about unsaved changes.
    Force(Box<Action>),
    Quit,
}

/// State of the interface that lives between frames.
pub struct UiState {
    pub search: String,
    pub object_cat: Option<Category>,
    pub spline_cat: Option<Category>,
    pub new_map: Option<crate::ui_newmap::NewMapForm>,
    pub help_open: bool,
    pub about_open: bool,
    pub texture_picker: Option<String>,
    pub maps: Vec<MapEntry>,
    /// Waiting for an answer about unsaved changes before doing this.
    pub confirm: Option<Action>,
    /// Help for what the mouse is over, shown in the status bar.
    pub status_help: Option<String>,
    pub welcome_pick: Option<PathBuf>,
    pub minimap: crate::ui_minimap::Minimap,
    pub check: crate::check::Checker,
    /// The window of the map's earlier versions (backups of its saves).
    pub versions_open: bool,
    /// The preferences' starred and recently used assets, for the catalogue.
    pub favourites: Vec<String>,
    pub recent_assets: Vec<String>,
    /// The map's `picture.jpg` as shown in the map's properties, with its file time.
    map_picture: Option<(PathBuf, Option<std::time::SystemTime>, Option<egui::TextureHandle>)>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            search: String::new(),
            object_cat: None,
            spline_cat: None,
            new_map: None,
            help_open: false,
            about_open: false,
            texture_picker: None,
            maps: Vec::new(),
            confirm: None,
            status_help: None,
            welcome_pick: None,
            minimap: crate::ui_minimap::Minimap::new(),
            check: Default::default(),
            versions_open: false,
            favourites: Vec::new(),
            recent_assets: Vec::new(),
            map_picture: None,
        }
    }
}

/// Facts for the status bar.
#[derive(Default)]
pub struct Status {
    pub cursor: Option<glam::DVec3>,
    pub loading: usize,
    pub gpu: String,
    pub cam: Option<crate::ui_minimap::CamView>,
}

pub fn style(ctx: &egui::Context) {
    ctx.set_fonts(icons::fonts());
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.faint_bg_color = Color32::from_rgb(40, 41, 45);
    v.extreme_bg_color = BG_FIELD;
    v.text_edit_bg_color = Some(BG_FIELD);
    v.code_bg_color = BG_DARK;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(3);
    v.window_shadow = egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(100) };
    v.selection.bg_fill = SELECT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_fill = BG;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(24, 24, 27));
    v.widgets.inactive.bg_fill = Color32::from_rgb(56, 58, 63);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(52, 54, 59);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(24, 24, 27));
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(66, 68, 74);
    v.widgets.hovered.bg_fill = Color32::from_rgb(70, 72, 78);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(90, 92, 100));
    v.widgets.active.weak_bg_fill = Color32::from_rgb(78, 80, 88);
    v.widgets.active.bg_fill = Color32::from_rgb(78, 80, 88);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(3);
    }
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.spacing.item_spacing = Vec2::new(6.0, 4.0);
        s.spacing.button_padding = Vec2::new(7.0, 3.0);
        s.spacing.interact_size = Vec2::new(36.0, 22.0);
        s.spacing.slider_width = 150.0;
        s.spacing.menu_margin = Margin::same(4);
        s.spacing.window_margin = Margin::same(10);
        use egui::{FontId, TextStyle};
        s.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(11.0));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(16.0));
        s.interaction.tooltip_delay = 10.0;
    });
    ctx.set_theme(egui::Theme::Dark);
}

/// Note help for the status bar when the mouse is over `r`.
fn help(st: &mut UiState, r: &egui::Response, text: impl Into<String>) {
    if r.hovered() {
        st.status_help = Some(text.into());
    }
}

/// A flat toolbar button with an icon; `on` draws it pressed.
fn tool_button(ui: &mut egui::Ui, icon: Icon, on: bool, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(30.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let fill = if on {
        Color32::from_rgb(70, 74, 82)
    } else if resp.hovered() && enabled {
        Color32::from_rgb(56, 58, 64)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(3), fill);
    if on {
        ui.painter().rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, Color32::from_rgb(98, 102, 112)), egui::StrokeKind::Inside);
        ui.painter().line_segment([rect.left_bottom() + Vec2::new(4.0, -1.5), rect.right_bottom() + Vec2::new(-4.0, -1.5)], Stroke::new(2.0, ACCENT));
    }
    let fg = if !enabled {
        Color32::from_gray(85)
    } else if on {
        Color32::WHITE
    } else {
        Color32::from_gray(205)
    };
    icons::paint(ui.painter(), icon, rect.center(), 20.0, fg);
    resp
}

/// A check box drawn with Material Symbols' own boxes (egui's are drawn with lines).
pub(crate) fn check(ui: &mut egui::Ui, on: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let galley = text.into().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let (icon, gap) = (18.0, 6.0);
    let size = Vec2::new(icon + gap + galley.size().x, galley.size().y.max(icon + 2.0));
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let vis = ui.style().interact(&resp);
    let col = if *on { ACCENT } else { vis.fg_stroke.color };
    icons::paint(ui.painter(), if *on { Icon::CheckOn } else { Icon::CheckOff }, egui::pos2(rect.left() + icon * 0.5, rect.center().y), icon + 2.0, col);
    let text_color = vis.text_color();
    ui.painter().galley(egui::pos2(rect.left() + icon + gap, rect.center().y - galley.size().y * 0.5), galley, text_color);
    resp
}

/// A radio button drawn with Material Symbols' own circles.
fn radio(ui: &mut egui::Ui, on: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let galley = text.into().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let (icon, gap) = (18.0, 6.0);
    let size = Vec2::new(icon + gap + galley.size().x, galley.size().y.max(icon + 2.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let vis = ui.style().interact(&resp);
    let col = if on { ACCENT } else { vis.fg_stroke.color };
    icons::paint(ui.painter(), if on { Icon::RadioOn } else { Icon::RadioOff }, egui::pos2(rect.left() + icon * 0.5, rect.center().y), icon + 2.0, col);
    let text_color = vis.text_color();
    ui.painter().galley(egui::pos2(rect.left() + icon + gap, rect.center().y - galley.size().y * 0.5), galley, text_color);
    resp
}

/// The open / closed arrow of a collapsing group, as Material Symbols draws it.
fn group_icon(ui: &mut egui::Ui, openness: f32, resp: &egui::Response) {
    let col = ui.style().interact(resp).fg_stroke.color;
    icons::paint(ui.painter(), if openness > 0.5 { Icon::ExpandMore } else { Icon::ChevronRight }, resp.rect.center(), 18.0, col);
}

/// The drop-down arrow of a combo box, as Material Symbols draws it.
fn combo_icon(ui: &egui::Ui, rect: egui::Rect, vis: &egui::style::WidgetVisuals, _open: bool) {
    icons::paint(ui.painter(), Icon::ExpandMore, rect.center(), 18.0, vis.fg_stroke.color);
}

pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
    Frame::new().fill(HEADER).inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title.to_uppercase()).small().strong().color(Color32::from_gray(190)));
    });
}

fn panel_frame() -> Frame {
    Frame::new().fill(BG).inner_margin(Margin::ZERO).stroke(Stroke::new(1.0, BORDER))
}

fn menu_item(ui: &mut egui::Ui, icon: Option<Icon>, text: &str, shortcut: &str, enabled: bool) -> bool {
    let label = match icon {
        Some(i) => egui::WidgetText::from(icons::labelled(i, text)),
        None => egui::WidgetText::from(format!("      {text}")),
    };
    let b = egui::Button::new(label).shortcut_text(shortcut);
    ui.add_enabled(enabled, b).clicked()
}

fn lang_menu(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    ui.menu_button(tr("Language", "Язык"), |ui| {
        for (l, n) in [(Lang::En, "English"), (Lang::Ru, "Русский")] {
            if radio(ui, i18n::get() == l, n).clicked() {
                actions.push(Action::SetLang(l));
            }
        }
    });
    ui.menu_button(tr("Interface size", "Размер интерфейса"), |ui| {
        let now = ui.ctx().zoom_factor();
        for z in [0.8f32, 0.9, 1.0, 1.1, 1.25, 1.5] {
            if radio(ui, (now - z).abs() < 0.01, format!("{:.0} %", z * 100.0)).clicked() {
                actions.push(Action::SetScale(z));
            }
        }
    });
}

// ---------------------------------------------------------------------------- start page

pub fn welcome(root: &mut egui::Ui, st: &mut UiState, prefs: &Prefs, omsi: Option<&std::path::Path>, actions: &mut Vec<Action>) {
    egui::Panel::top("menu").frame(Frame::new().fill(BG_DARK).inner_margin(Margin::symmetric(6, 2))).show(root, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("File", "Файл"), |ui| {
                if menu_item(ui, Some(Icon::NewFile), tr("New map…", "Новая карта…"), "", omsi.is_some()) {
                    st.new_map = Some(Default::default());
                }
                if menu_item(ui, Some(Icon::Open), tr("Open map…", "Открыть карту…"), "", omsi.is_some()) {
                    actions.push(Action::OpenDialog);
                }
                if menu_item(ui, Some(Icon::Import), tr("Import assets…", "Импорт ассетов…"), "", omsi.is_some()) {
                    actions.push(Action::Import);
                }
                ui.separator();
                if menu_item(ui, Some(Icon::Settings), tr("OMSI 2 folder…", "Папка OMSI 2…"), "", true) {
                    actions.push(Action::ChooseOmsi);
                }
                ui.separator();
                if menu_item(ui, None, tr("Quit", "Выход"), "", true) {
                    actions.push(Action::Quit);
                }
            });
            ui.menu_button(tr("View", "Вид"), |ui| lang_menu(ui, actions));
            ui.menu_button(tr("Help", "Справка"), |ui| {
                if menu_item(ui, Some(Icon::Info), tr("About openMapper", "О программе"), "", true) {
                    st.about_open = true;
                }
            });
        });
    });
    egui::Panel::bottom("status").exact_size(22.0).frame(Frame::new().fill(BG_DARK).inner_margin(Margin::symmetric(8, 3))).show(root, |ui| {
        ui.horizontal(|ui| {
            match omsi {
                Some(p) => ui.label(RichText::new(format!("OMSI 2: {}", p.display())).small().color(TEXT_DIM)),
                None => ui.label(RichText::new(tr("OMSI 2 not found", "OMSI 2 не найдена")).small().color(Color32::from_rgb(230, 150, 90))),
            };
        });
    });
    egui::CentralPanel::default().frame(Frame::new().fill(Color32::from_rgb(30, 31, 34))).show(root, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.5 - 230.0).max(20.0));
            Frame::new().fill(BG).stroke(Stroke::new(1.0, BORDER)).corner_radius(CornerRadius::same(4)).show(ui, |ui| {
                ui.set_width(660.0);
                Frame::new().fill(HEADER).inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(Icon::Map.text(30.0).color(ACCENT));
                        ui.vertical(|ui| {
                            ui.label(RichText::new("openMapper").size(20.0).strong().color(Color32::WHITE));
                            ui.label(RichText::new(format!("{} · v{}", tr("Map editor for openOMSI", "Редактор карт для openOMSI"), env!("CARGO_PKG_VERSION"))).small().color(TEXT_DIM));
                        });
                    });
                });
                Frame::new().inner_margin(Margin::same(14)).show(ui, |ui| {
                    if omsi.is_none() {
                        ui.label(tr("openMapper builds maps from the content of an installed OMSI 2. Choose its folder (the one with Omsi.exe, maps and Vehicles).", "openMapper строит карты из контента установленной OMSI 2. Укажите её папку (где лежат Omsi.exe, maps и Vehicles)."));
                        ui.add_space(8.0);
                        if ui.button(icons::labelled(Icon::Open, tr("Choose OMSI 2 folder…", "Выбрать папку OMSI 2…"))).clicked() {
                            actions.push(Action::ChooseOmsi);
                        }
                        return;
                    }
                    ui.horizontal_top(|ui| {
                        // the maps
                        ui.vertical(|ui| {
                            ui.set_width(430.0);
                            ui.label(RichText::new(tr("Maps", "Карты")).strong());
                            Frame::new().fill(BG_FIELD).stroke(Stroke::new(1.0, BORDER)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                egui::ScrollArea::vertical().min_scrolled_height(300.0).max_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                                    // a recent map of the installation that was since saved opens
                                    // as its copy in the content folder, so it is listed once
                                    let folder_of = |p: &std::path::Path| p.parent().and_then(|d| d.file_name()).map(|f| f.to_string_lossy().to_lowercase());
                                    let mut recent: Vec<PathBuf> = Vec::new();
                                    for p in prefs.recent.iter().map(PathBuf::from).filter(|p| p.is_file()) {
                                        let p = st.maps.iter().find(|m| m.in_content && folder_of(&m.cfg) == folder_of(&p) && !p.starts_with(m.cfg.parent().unwrap_or(&m.cfg))).map(|m| m.cfg.clone()).unwrap_or(p);
                                        if !recent.contains(&p) {
                                            recent.push(p);
                                        }
                                    }
                                    let mut rows: Vec<(String, PathBuf, &str)> = Vec::new();
                                    for p in &recent {
                                        let name = st.maps.iter().find(|m| m.cfg == *p).map(|m| m.name.clone()).unwrap_or_else(|| p.parent().and_then(|d| d.file_name()).map(|f| f.to_string_lossy().to_string()).unwrap_or_default());
                                        rows.push((name, p.clone(), tr("recent", "недавняя")));
                                    }
                                    for m in &st.maps {
                                        if recent.contains(&m.cfg) {
                                            continue;
                                        }
                                        rows.push((m.name.clone(), m.cfg.clone(), if m.in_content { tr("content folder", "папка контента") } else { tr("OMSI 2 (saved as copy)", "OMSI 2 (сохр. копией)") }));
                                    }
                                    for (i, (name, cfg, kind)) in rows.iter().enumerate() {
                                        let on = st.welcome_pick.as_ref() == Some(cfg);
                                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
                                        let fill = if on { SELECT } else if i % 2 == 1 { Color32::from_rgb(28, 29, 32) } else { Color32::TRANSPARENT };
                                        ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
                                        icons::paint(ui.painter(), Icon::Map, rect.left_center() + Vec2::new(16.0, 0.0), 18.0, if on { Color32::WHITE } else { TEXT_DIM });
                                        ui.painter().text(rect.left_center() + Vec2::new(30.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), Color32::from_gray(230));
                                        ui.painter().text(rect.right_center() - Vec2::new(8.0, 0.0), egui::Align2::RIGHT_CENTER, *kind, egui::FontId::proportional(11.0), TEXT_DIM);
                                        if resp.clicked() {
                                            st.welcome_pick = Some(cfg.clone());
                                        }
                                        if resp.double_clicked() {
                                            actions.push(Action::Open(cfg.clone()));
                                        }
                                    }
                                });
                            });
                        });
                        // the actions
                        ui.vertical(|ui| {
                            ui.add_space(19.0);
                            let w = Vec2::new(180.0, 26.0);
                            if ui.add_enabled(st.welcome_pick.is_some(), egui::Button::new(icons::labelled(Icon::Open, tr("Open", "Открыть"))).min_size(w)).clicked() {
                                if let Some(p) = st.welcome_pick.clone() {
                                    actions.push(Action::Open(p));
                                }
                            }
                            if ui.add(egui::Button::new(icons::labelled(Icon::NewFile, tr("New map…", "Новая карта…"))).min_size(w)).clicked() {
                                st.new_map = Some(Default::default());
                            }
                            if ui.add(egui::Button::new(icons::labelled(Icon::Open, tr("Browse…", "Обзор…"))).min_size(w)).clicked() {
                                actions.push(Action::OpenDialog);
                            }
                            ui.separator();
                            if ui.add(egui::Button::new(icons::labelled(Icon::Import, tr("Import assets…", "Импорт ассетов…"))).min_size(w)).clicked() {
                                actions.push(Action::Import);
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr("Double-click a map to open it. Maps of OMSI 2 are saved as a copy into openOMSI's content folder; the original stays untouched.", "Двойной клик — открыть карту. Карты OMSI 2 сохраняются копией в папку контента openOMSI; оригинал не меняется.")).small().color(TEXT_DIM));
                });
            });
        });
    });
    about(root.ctx(), st);
}

fn about(ctx: &egui::Context, st: &mut UiState) {
    let mut open = st.about_open;
    egui::Window::new(tr("About openMapper", "О программе")).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.label(RichText::new(format!("openMapper {}", env!("CARGO_PKG_VERSION"))).strong());
        ui.label(tr("A simple map editor for openOMSI.", "Простой редактор карт для openOMSI."));
        ui.hyperlink_to("openomsi.org", "https://openomsi.org/");
        ui.label(RichText::new("Icons: Material Symbols (Google, Apache 2.0)").small().color(TEXT_DIM));
    });
    st.about_open = open;
}

// ---------------------------------------------------------------------------- editor

pub struct EditorUi<'a> {
    pub ed: &'a mut Editor,
    pub lib: &'a Library,
    pub thumbs: &'a HashMap<String, Option<Thumb>>,
    pub wanted: &'a mut Vec<(String, bool)>,
    pub swatches: &'a HashMap<String, egui::TextureHandle>,
    pub wanted_swatches: &'a mut Vec<String>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub game_found: bool,
    pub texture_files: &'a [String],
    pub prefs: &'a Prefs,
    pub status: Status,
}

/// The editor's interface; returns the 3D view's rectangle and whether the mouse is over
/// it (and not over a window on top of it).
pub fn editor(root: &mut egui::Ui, st: &mut UiState, u: EditorUi, actions: &mut Vec<Action>) -> (egui::Rect, bool) {
    let EditorUi { ed, lib, thumbs, wanted, swatches, wanted_swatches, can_undo, can_redo, game_found, texture_files, prefs, status } = u;
    menu_bar(root, st, ed, can_undo, can_redo, game_found, prefs, actions);
    toolbar(root, st, ed, can_undo, can_redo, game_found, actions);
    // (the panels below fill in the help; the status bar shows last frame's)
    let help_line = st.status_help.take();
    status_bar(root, help_line, ed, &status);
    egui::Panel::left("tool panel").resizable(true).default_size(290.0).size_range(240.0..=480.0).frame(panel_frame()).show(root, |ui| {
        tool_panel(ui, st, ed, lib, thumbs, wanted, swatches, wanted_swatches, actions);
    });
    egui::Panel::right("properties").resizable(true).default_size(270.0).size_range(220.0..=420.0).frame(panel_frame()).show(root, |ui| {
        // (a road's rows of objects can make it long)
        egui::ScrollArea::vertical().id_salt("properties scroll").auto_shrink([false, false]).show(ui, |ui| {
            properties(ui, st, ed, lib, actions);
        });
    });
    let mut view = (egui::Rect::NOTHING, false);
    egui::CentralPanel::default().frame(Frame::NONE).show(root, |ui| {
        let rect = ui.max_rect();
        let resp = ui.interact(rect, Id::new("viewport"), Sense::hover());
        view = (rect, resp.contains_pointer());
        ui.painter().rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
        if let (Some(doc), Some(cam)) = (&ed.doc, status.cam) {
            if let Some(p) = st.minimap.show(ui, rect, doc, lib, cam) {
                actions.push(Action::LookAt(p));
            }
        }
    });
    let ctx = root.ctx().clone();
    texture_picker(&ctx, st, texture_files, swatches, wanted_swatches, actions);
    controls(&ctx, st);
    about(&ctx, st);
    st.check.window(&ctx, ed, lib, actions);
    if let Some(doc) = &ed.doc {
        crate::doc::safe::versions_window(&ctx, &mut st.versions_open, &doc.dir, actions);
    }
    view
}

#[allow(clippy::too_many_arguments)]
fn menu_bar(root: &mut egui::Ui, st: &mut UiState, ed: &mut Editor, can_undo: bool, can_redo: bool, game_found: bool, prefs: &Prefs, actions: &mut Vec<Action>) {
    egui::Panel::top("menu").frame(Frame::new().fill(BG_DARK).inner_margin(Margin::symmetric(6, 2))).show(root, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("File", "Файл"), |ui| {
                if menu_item(ui, Some(Icon::NewFile), tr("New map…", "Новая карта…"), "", true) {
                    st.new_map = Some(Default::default());
                }
                if menu_item(ui, Some(Icon::Open), tr("Open map…", "Открыть карту…"), "", true) {
                    actions.push(Action::OpenDialog);
                }
                ui.menu_button(icons::labelled(Icon::Map, tr("Recent maps", "Недавние карты")), |ui| {
                    let recent: Vec<PathBuf> = prefs.recent.iter().map(PathBuf::from).filter(|p| p.is_file()).collect();
                    if recent.is_empty() {
                        ui.label(RichText::new(tr("(none)", "(нет)")).color(TEXT_DIM));
                    }
                    for p in recent {
                        let name = p.parent().and_then(|d| d.file_name()).map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.button(name).clicked() {
                            actions.push(Action::Open(p));
                        }
                    }
                });
                ui.separator();
                if menu_item(ui, Some(Icon::Save), tr("Save", "Сохранить"), "Ctrl+S", true) {
                    actions.push(Action::Save);
                }
                if menu_item(ui, Some(Icon::Copy), tr("Export map for sharing…", "Экспорт карты для обмена…"), "", true) {
                    actions.push(Action::Export);
                }
                if menu_item(ui, Some(Icon::Import), tr("Import assets…", "Импорт ассетов…"), "", true) {
                    actions.push(Action::Import);
                }
                if menu_item(ui, Some(Icon::History), tr("Earlier versions…", "Прошлые версии…"), "", true) {
                    st.versions_open = true;
                }
                ui.separator();
                if menu_item(ui, None, tr("Close map", "Закрыть карту"), "", true) {
                    actions.push(Action::Close);
                }
                if menu_item(ui, None, tr("Quit", "Выход"), "", true) {
                    actions.push(Action::Quit);
                }
            });
            ui.menu_button(tr("Edit", "Правка"), |ui| {
                let undo = match ed.history.undo_label() {
                    Some(l) => format!("{} «{l}»", tr("Undo", "Отменить")),
                    None => tr("Undo", "Отменить").to_string(),
                };
                if menu_item(ui, Some(Icon::Undo), &undo, "Ctrl+Z", can_undo) {
                    actions.push(Action::Undo);
                }
                if menu_item(ui, Some(Icon::Redo), tr("Redo", "Повторить"), "Ctrl+Shift+Z", can_redo) {
                    actions.push(Action::Redo);
                }
                ui.separator();
                let any = ed.selected.is_some();
                if menu_item(ui, None, tr("Cut", "Вырезать"), "Ctrl+X", any) {
                    actions.push(Action::Cut);
                }
                if menu_item(ui, Some(Icon::Copy), tr("Copy", "Копировать"), "Ctrl+C", any) {
                    actions.push(Action::Copy);
                }
                if menu_item(ui, Some(Icon::Paste), tr("Paste", "Вставить"), "Ctrl+V", ed.clip.is_some()) {
                    actions.push(Action::Paste);
                }
                if menu_item(ui, None, tr("Duplicate", "Дублировать"), "Ctrl+D", any) {
                    actions.push(Action::Duplicate);
                }
                if menu_item(ui, Some(Icon::SelectAll), tr("Select all", "Выбрать всё"), "Ctrl+A", true) {
                    actions.push(Action::SelectAll);
                }
                ui.separator();
                if menu_item(ui, Some(Icon::Delete), tr("Delete", "Удалить"), "Del", ed.selected.is_some()) {
                    ed.delete_selected();
                }
                if menu_item(ui, None, tr("Deselect", "Снять выделение"), "Esc", ed.selected.is_some()) {
                    ed.select(None);
                }
            });
            ui.menu_button(tr("View", "Вид"), |ui| {
                check(ui, &mut ed.show_grid, tr("Tile grid", "Сетка тайлов"));
                let icon = if ed.show_technical { Icon::Visible } else { Icon::Hidden };
                check(ui, &mut ed.show_technical, icons::labelled(icon, tr("What the game does not show", "То, чего не видно в игре")));
                check(ui, &mut ed.show_lanes, icons::labelled(Icon::Route, tr("AI lanes", "Полосы ИИ")));
                check(ui, &mut st.minimap.open, icons::labelled(Icon::Map, tr("Overview map (M)", "Обзорная карта (M)")));
                if menu_item(ui, Some(Icon::Focus), tr("Go to selection", "К выделенному"), "F", ed.selected.is_some()) {
                    actions.push(Action::FocusSelection);
                }
                ui.separator();
                lang_menu(ui, actions);
                ui.menu_button(tr("Graphics quality", "Качество графики"), |ui| {
                    let now = crate::render::quality::Quality::from_code(&prefs.graphics);
                    for q in crate::render::quality::Quality::ALL {
                        if radio(ui, now == q, q.label()).clicked() {
                            actions.push(Action::SetGraphics(q));
                        }
                    }
                });
            });
            ui.menu_button(tr("Tools", "Инструменты"), |ui| {
                for t in Tool::ALL {
                    if ui.add(egui::Button::new(icons::labelled(t.icon(), t.label())).shortcut_text(t.key()).selected(ed.tool == t)).clicked() {
                        ed.set_tool(t);
                    }
                }
            });
            ui.menu_button(tr("Map", "Карта"), |ui| {
                if menu_item(ui, Some(Icon::Settings), tr("Map properties", "Свойства карты"), "", true) {
                    ed.selected = None;
                    ed.set_tool(Tool::Select);
                }
                if menu_item(ui, Some(Icon::Layers), tr("Ground textures…", "Текстуры земли…"), "", true) {
                    ed.set_tool(Tool::Paint);
                }
                if menu_item(ui, Some(Icon::CheckCircle), tr("Check map…", "Проверить карту…"), "", true) {
                    st.check.open = true;
                    st.check.issues = None;
                }
                if menu_item(ui, Some(Icon::Bus), tr("Lines and timetable…", "Линии и расписание…"), "L", true) {
                    actions.push(Action::Timetable);
                }
                if menu_item(ui, Some(Icon::Traffic), tr("Traffic and AI…", "Трафик и ИИ…"), "T", true) {
                    actions.push(Action::Traffic);
                }
                ui.separator();
                if menu_item(ui, Some(Icon::Play), tr("Test drive in openOMSI", "Тест-драйв в openOMSI"), "F5", game_found) {
                    actions.push(Action::Play);
                }
            });
            ui.menu_button(tr("Help", "Справка"), |ui| {
                if menu_item(ui, Some(Icon::CheckCircle), tr("Getting started", "С чего начать"), "", true) {
                    actions.push(Action::Guide);
                }
                if menu_item(ui, Some(Icon::Help), tr("Controls", "Управление"), "", true) {
                    st.help_open = true;
                }
                if menu_item(ui, Some(Icon::Info), tr("About openMapper", "О программе"), "", true) {
                    st.about_open = true;
                }
            });
        });
    });
}

fn toolbar(root: &mut egui::Ui, st: &mut UiState, ed: &mut Editor, can_undo: bool, can_redo: bool, game_found: bool, actions: &mut Vec<Action>) {
    egui::Panel::top("toolbar").frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(6, 3)).stroke(Stroke::new(1.0, BORDER))).show(root, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let r = tool_button(ui, Icon::NewFile, false, true);
            help(st, &r, tr("New map", "Новая карта"));
            if r.clicked() {
                st.new_map = Some(Default::default());
            }
            let r = tool_button(ui, Icon::Open, false, true);
            help(st, &r, tr("Open a map", "Открыть карту"));
            if r.clicked() {
                actions.push(Action::OpenDialog);
            }
            let dirty = ed.doc.as_ref().map(|d| d.is_dirty()).unwrap_or(false);
            let r = tool_button(ui, Icon::Save, false, true);
            if dirty {
                ui.painter().circle_filled(r.rect.right_top() + Vec2::new(-6.0, 6.0), 3.0, ACCENT);
            }
            help(st, &r, if dirty { tr("Save (Ctrl+S) — there are unsaved changes", "Сохранить (Ctrl+S) — есть несохранённые изменения") } else { tr("Save (Ctrl+S)", "Сохранить (Ctrl+S)") });
            if r.clicked() {
                actions.push(Action::Save);
            }
            let r = tool_button(ui, Icon::Import, false, true);
            help(st, &r, tr("Import assets: a mod folder or .zip, a .obj / .x / .o3d model (or drop files on the window)", "Импорт ассетов: папка мода или .zip, модель .obj / .x / .o3d (или перетащите файлы в окно)"));
            if r.clicked() {
                actions.push(Action::Import);
            }
            ui.separator();
            let r = tool_button(ui, Icon::Undo, false, can_undo);
            help(st, &r, format!("{} (Ctrl+Z){}", tr("Undo", "Отменить"), ed.history.undo_label().map(|l| format!(": {l}")).unwrap_or_default()));
            if r.clicked() {
                actions.push(Action::Undo);
            }
            let r = tool_button(ui, Icon::Redo, false, can_redo);
            help(st, &r, tr("Redo (Ctrl+Shift+Z)", "Повторить (Ctrl+Shift+Z)"));
            if r.clicked() {
                actions.push(Action::Redo);
            }
            ui.separator();
            for t in Tool::ALL {
                let r = tool_button(ui, t.icon(), ed.tool == t, true);
                help(st, &r, format!("{} ({})", t.label(), t.key()));
                if r.clicked() {
                    ed.set_tool(t);
                }
            }
            ui.separator();
            let r = tool_button(ui, Icon::Tiles, ed.show_grid, true);
            help(st, &r, tr("Tile grid (G)", "Сетка тайлов (G)"));
            if r.clicked() {
                ed.show_grid = !ed.show_grid;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let r = ui.add_enabled(game_found, egui::Button::new(icons::labelled(Icon::Play, tr("Test drive", "Тест-драйв"))));
                help(st, &r, if game_found { tr("Save the map and drive it in openOMSI (F5)", "Сохранить карту и проехать её в openOMSI (F5)") } else { tr("openOMSI was not found: install it to test maps", "openOMSI не найден: установите его, чтобы проверять карты") });
                if r.clicked() {
                    actions.push(Action::Play);
                }
                if let Some(d) = &ed.doc {
                    ui.label(RichText::new(d.display_name()).color(TEXT_DIM));
                }
            });
        });
    });
}

fn status_bar(root: &mut egui::Ui, help_line: Option<String>, ed: &mut Editor, status: &Status) {
    ed.toasts.retain(|t| t.at.elapsed().as_secs_f32() < 8.0);
    egui::Panel::bottom("status").exact_size(22.0).frame(Frame::new().fill(BG_DARK).inner_margin(Margin::symmetric(8, 3))).show(root, |ui| {
        ui.horizontal(|ui| {
            // the help of what the mouse is over, else the newest message, else the tool's
            let (text, color) = match (ed.toasts.last(), help_line) {
                (_, Some(h)) => (h, Color32::from_gray(210)),
                (Some(t), None) => (t.text.clone(), if t.error { Color32::from_rgb(240, 120, 100) } else { Color32::from_rgb(150, 210, 140) }),
                (None, None) => (tool_hint(ed).to_string(), TEXT_DIM),
            };
            // (the facts on the right first; the help gets what is left, cut short)
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(&status.gpu).small().color(Color32::from_gray(95)));
                ui.separator();
                if status.loading > 0 {
                    ui.label(RichText::new(format!("{} {}", tr("loading", "загрузка"), status.loading)).small().color(TEXT_DIM));
                    ui.spinner();
                    ui.separator();
                }
                if let Some(doc) = &ed.doc {
                    let objects: usize = doc.tiles.values().map(|t| t.tile.objects.len()).sum();
                    let splines: usize = doc.tiles.values().map(|t| t.tile.splines.len()).sum();
                    ui.label(RichText::new(format!("{} {} · {} {} · {} {}", doc.tiles.len(), tr("tiles", "тайлов"), splines, tr("splines", "сплайнов"), objects, tr("objects", "объектов"))).small().color(TEXT_DIM));
                    ui.separator();
                }
                if let Some(c) = status.cursor {
                    let t = crate::doc::tile_of(c.truncate());
                    ui.label(RichText::new(format!("X {:>8.1}   Y {:>8.1}   Z {:>6.1}   {} {},{}", c.x, c.y, c.z, tr("tile", "тайл"), t.0, t.1)).small().monospace().color(TEXT_DIM));
                }
                if ed.doc.is_some() {
                    ui.separator();
                    crate::ui_edit::snap_controls(ui, ed);
                }
                ui.separator();
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let r = ui.add(egui::Label::new(RichText::new(&text).small().color(color)).truncate());
                    r.on_hover_text(&text);
                });
            });
        });
    });
}

fn tool_hint(ed: &Editor) -> &'static str {
    match ed.tool {
        Tool::Select if ed.single_spline().is_some() => tr("Drag a white dot: an end moves (joined roads follow), the middle bends the road · drag the road to move it · Shift+click: add to the selection", "Тяните белую точку: конец двигается (соседние дороги следуют), середина изгибает дорогу · тянуть дорогу — сдвиг · Shift+клик — добавить к выделению"),
        Tool::Select => tr("Click to select, Shift+click to add · Shift+drag on empty ground: box · drag the selection to move it, empty ground to move the map · Ctrl+C / X / V / D · right button: look + WASD/QE fly · wheel: zoom · F: focus", "Клик — выбрать, Shift+клик — добавить · Shift+тянуть по пустой земле — рамка · тянуть выделенное — сдвиг, пустую землю — двигать карту · Ctrl+C / X / V / D · ПКМ — обзор + полёт WASD/QE · колесо — зум · F — к выделенному"),
        Tool::Roads if ed.road_type.is_none() => tr("Choose a road type in the list on the left", "Выберите тип дороги в списке слева"),
        Tool::Roads => tr("Click where the road starts, click again where it goes (it keeps going) · white dots: road ends to join · right-click / Esc: stop", "Клик — начало дороги, ещё клик — куда она идёт (дальше продолжается) · белые точки — концы для стыковки · правый клик / Esc — стоп"),
        Tool::Objects if ed.object_type.is_none() => tr("Choose an object in the list on the left", "Выберите объект в списке слева"),
        Tool::Objects => tr("Click to place · R / Shift+R turn · right-click puts the object away", "Клик — поставить · R / Shift+R — поворот · правый клик — убрать объект"),
        Tool::Logic if ed.logic == LogicMode::Paths => tr("Click where the path starts, click where it goes (it keeps going) · it snaps to lane ends and along lanes · right-click / Esc: stop", "Клик — начало пути, ещё клик — куда он идёт (дальше продолжается) · прилипает к концам и вдоль полос · правый клик / Esc — стоп"),
        Tool::Logic if ed.logic == LogicMode::Parking && ed.parking_row => tr("Click the start of a row, then its end · R turns the cars · right-click: cancel", "Клик — начало ряда, потом его конец · R — повернуть машины · правый клик — отмена"),
        Tool::Logic => tr("Choose what to place in the list on the left, then click in the view · R / Shift+R turn", "Выберите, что ставить, в списке слева, потом клик в окне · R / Shift+R — поворот"),
        Tool::Terrain | Tool::Paint => tr("Hold the left button and drag · [ ] or Shift+wheel: brush size", "Зажмите левую кнопку и ведите · [ ] или Shift+колесо — размер кисти"),
        Tool::Tiles => tr("Click beside the map to add a tile · right-click a tile to remove it", "Клик рядом с картой — добавить тайл · правый клик по тайлу — удалить"),
    }
}

// ---------------------------------------------------------------------------- left panel

#[allow(clippy::too_many_arguments)]
fn tool_panel(
    ui: &mut egui::Ui,
    st: &mut UiState,
    ed: &mut Editor,
    lib: &Library,
    thumbs: &HashMap<String, Option<Thumb>>,
    wanted: &mut Vec<(String, bool)>,
    swatches: &HashMap<String, egui::TextureHandle>,
    wanted_swatches: &mut Vec<String>,
    actions: &mut Vec<Action>,
) {
    section(ui, ed.tool.label());
    Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| match ed.tool {
        Tool::Roads => catalogue(ui, st, ed, lib, thumbs, wanted, Kind::Spline, actions),
        Tool::Objects => catalogue(ui, st, ed, lib, thumbs, wanted, Kind::Object, actions),
        Tool::Logic => logic_panel(ui, st, ed, lib, thumbs, wanted, actions),
        Tool::Terrain => terrain_panel(ui, st, ed),
        Tool::Paint => paint_panel(ui, st, ed, swatches, wanted_swatches, actions),
        Tool::Tiles => {
            let n = ed.doc.as_ref().map(|d| d.tiles.len()).unwrap_or(0);
            ui.label(tr("Click beside the map to add a 300 × 300 m tile. Right-click a tile to remove it.", "Клик рядом с картой — добавить тайл 300 × 300 м. Правый клик по тайлу — удалить."));
            ui.add_space(6.0);
            egui::Grid::new("tiles").num_columns(2).show(ui, |ui| {
                ui.label(RichText::new(tr("Tiles", "Тайлов")).color(TEXT_DIM));
                ui.label(n.to_string());
                ui.end_row();
                ui.label(RichText::new(tr("Area", "Площадь")).color(TEXT_DIM));
                ui.label(format!("{:.2} km²", n as f64 * crate::doc::tile_size().powi(2) / 1e6));
                ui.end_row();
            });
        }
        Tool::Select => {
            ui.label(RichText::new(tr("Click an object or a road in the view to see and change it on the right.", "Кликните объект или дорогу в окне вида — свойства появятся справа.")).color(TEXT_DIM));
            ui.add_space(8.0);
            ui.label(RichText::new(tr("Tools", "Инструменты")).strong());
            for t in Tool::ALL.iter().skip(1) {
                if ui.add(egui::Button::new(icons::labelled(t.icon(), t.label())).shortcut_text(t.key()).min_size(Vec2::new(ui.available_width(), 24.0))).clicked() {
                    ed.set_tool(*t);
                }
            }
        }
    });
}

fn catalogue(ui: &mut egui::Ui, st: &mut UiState, ed: &mut Editor, lib: &Library, thumbs: &HashMap<String, Option<Thumb>>, wanted: &mut Vec<(String, bool)>, kind: Kind, actions: &mut Vec<Action>) {
    catalogue_of(ui, st, ed, lib, thumbs, wanted, kind, None, actions)
}

/// The Logic tool: what the game uses but does not show, a tab for each kind.
fn logic_panel(ui: &mut egui::Ui, st: &mut UiState, ed: &mut Editor, lib: &Library, thumbs: &HashMap<String, Option<Thumb>>, wanted: &mut Vec<(String, bool)>, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for m in LogicMode::ALL {
            let on = ed.logic == m;
            let b = egui::Button::new(icons::labelled(m.icon(), m.label())).selected(on).min_size(Vec2::new(0.0, 26.0));
            if ui.add(b).clicked() {
                ed.set_logic(m);
            }
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new(ed.logic.hint()).small().color(TEXT_DIM));
    ui.add_space(4.0);
    match ed.logic {
        LogicMode::Parking => {
            check(ui, &mut ed.parking_row, tr("In a row along a line", "Рядом вдоль линии"));
            if ed.parking_row {
                ui.horizontal(|ui| {
                    ui.label(tr("Every", "Через каждые"));
                    ui.add(egui::DragValue::new(&mut ed.parking_step).range(2.0..=30.0).speed(0.1).suffix(" m"));
                });
            }
        }
        LogicMode::Paths => {
            // the two everyone needs, at the top
            ui.horizontal(|ui| {
                for (icon, label, file) in [(Icon::Traffic, tr("Cars", "Машины"), "Splines\\Marcel\\invis_street.sli"), (Icon::Walk, tr("People", "Люди"), "Splines\\invis_sidewalk.sli")] {
                    let Some(a) = lib.find(file) else { continue };
                    let on = ed.path_type.as_deref().map(|p| p.eq_ignore_ascii_case(&a.rel)).unwrap_or(false);
                    if ui.add(egui::Button::new(icons::labelled(icon, label)).selected(on).min_size(Vec2::new(90.0, 28.0))).clicked() {
                        ed.set_path_type(a.rel.clone());
                    }
                }
            });
            if ed.path_type.is_none() {
                if let Some(a) = lib.find("Splines\\Marcel\\invis_street.sli") {
                    ed.set_path_type(a.rel.clone());
                }
            }
        }
        _ => {}
    }
    check(ui, &mut ed.show_technical, tr("Show these in the other tools too", "Показывать это и в других инструментах"));
    check(ui, &mut ed.show_lanes, tr("Show the AI's lanes in the other tools too", "Показывать полосы ИИ и в других инструментах"));
    ui.add_space(4.0);
    let kind = if ed.logic == LogicMode::Paths { Kind::Spline } else { Kind::Object };
    catalogue_of(ui, st, ed, lib, thumbs, wanted, kind, Some(ed.logic), actions);
}

/// The asset list (with a Logic tab: only what belongs to it, and no category choice).
#[allow(clippy::too_many_arguments)]
fn catalogue_of(ui: &mut egui::Ui, st: &mut UiState, ed: &mut Editor, lib: &Library, thumbs: &HashMap<String, Option<Thumb>>, wanted: &mut Vec<(String, bool)>, kind: Kind, logic: Option<LogicMode>, actions: &mut Vec<Action>) {
    let belongs = |a: &crate::library::Asset| -> bool {
        match logic {
            None => true,
            Some(LogicMode::Start) => a.is_entry_point,
            Some(LogicMode::Stops) => a.is_bus_stop,
            Some(LogicMode::Parking) => a.is_car_park,
            Some(LogicMode::Paths) => a.category == Category::Technical && (a.lanes.0 > 0 || a.lanes.1 > 0),
            Some(LogicMode::Other) => a.category == Category::Technical && !a.is_car_park,
        }
    };
    let cats: &[Category] = if kind == Kind::Spline { &Category::SPLINES } else { &Category::OBJECTS };
    let cat = if kind == Kind::Spline { &mut st.spline_cat } else { &mut st.object_cat };
    let mut none: Option<Category> = None;
    let cat = if logic.is_some() { &mut none } else { cat };
    egui::Grid::new("filter").num_columns(2).spacing(Vec2::new(6.0, 4.0)).show(ui, |ui| {
        if logic.is_none() {
        ui.label(tr("Show", "Показать"));
        let current = cat.map(|c| c.label()).unwrap_or(tr("Everything", "Всё"));
        egui::ComboBox::from_id_salt(("cat", kind == Kind::Spline)).icon(combo_icon).width(ui.available_width()).selected_text(current).show_ui(ui, |ui| {
            ui.selectable_value(cat, None, tr("Everything", "Всё"));
            for c in cats {
                let n = lib.count(kind, *c);
                if n > 0 {
                    ui.selectable_value(cat, Some(*c), icons::labelled(c.icon(), &format!("{}  ({n})", c.label())));
                }
            }
        });
        ui.end_row();
        }
        ui.label(tr("Search", "Поиск"));
        ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(f32::INFINITY));
        ui.end_row();
    });
    if logic.is_some() {
    } else if kind == Kind::Spline {
        check(ui, &mut ed.follow_ground, tr("Shape the ground under the road", "Подгонять землю под дорогу"));
        crate::ui_edit::road_options(ui, ed);
        check(ui, &mut ed.auto_junctions, tr("Make junctions where roads meet", "Делать перекрёстки, где дороги встречаются"));
        check(ui, &mut ed.inherit_type, tr("Go on with the type of the road I start from", "Продолжать типом дороги, от которой начинаю"));
        let current = ed.road_type.as_deref().map(|r| lib.find(r).map(|a| a.name.clone()).unwrap_or_else(|| r.to_string()));
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Building with:", "Строится:")).color(TEXT_DIM));
            ui.label(RichText::new(current.unwrap_or_else(|| tr("nothing chosen", "ничего не выбрано").to_string())).strong().color(Color32::WHITE));
        });
    } else {
        check(ui, &mut ed.align_to_road, tr("Turn towards the nearest road", "Разворачивать к ближайшей дороге"));
    }
    let selected = if logic == Some(LogicMode::Paths) { ed.path_type.clone() } else if kind == Kind::Spline { ed.road_type.clone() } else { ed.object_type.clone() };
    let items: Vec<&crate::library::Asset> = lib.filter(kind, *cat, &st.search).filter(|a| belongs(a)).collect();
    ui.label(RichText::new(if lib.scanning { tr("reading OMSI 2…", "чтение OMSI 2…").to_string() } else { format!("{} {}", items.len(), tr("items", "шт.")) }).small().color(TEXT_DIM));
    let avail = ui.available_height();
    let searching = !st.search.trim().is_empty();
    Frame::new().fill(BG_FIELD).stroke(Stroke::new(1.0, BORDER)).show(ui, |ui| {
        ui.set_min_height(avail - 6.0);
        ui.set_width(ui.available_width());
        if items.is_empty() {
            ui.label(RichText::new(tr("Nothing found", "Ничего не найдено")).color(TEXT_DIM));
            return;
        }
        let mut row = |ui: &mut egui::Ui, a: &crate::library::Asset, i: usize, st: &mut UiState, ed: &mut Editor| {
            let h = 40.0;
            let on = selected.as_deref().map(|s| s.eq_ignore_ascii_case(&a.rel)).unwrap_or(false);
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
            let fill = if on {
                SELECT
            } else if resp.hovered() {
                Color32::from_gray(40)
            } else if i % 2 == 1 {
                Color32::from_rgb(26, 27, 30)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
            let img = egui::Rect::from_min_size(rect.min + Vec2::new(3.0, 2.0), Vec2::splat(h - 4.0));
            match thumbs.get(&a.rel.to_lowercase()) {
                Some(Some(t)) => {
                    egui::Image::new((t.id, img.size())).paint_at(ui, img);
                }
                Some(None) => {
                    icons::paint(ui.painter(), a.category.icon(), img.center(), 20.0, Color32::from_gray(90));
                }
                None => {
                    wanted.push((a.rel.clone(), kind == Kind::Spline));
                    ui.painter().rect_filled(img.shrink(11.0), CornerRadius::same(2), Color32::from_gray(40));
                }
            }
            ui.painter().text(rect.min + Vec2::new(h + 6.0, 12.0), egui::Align2::LEFT_CENTER, &a.name, egui::FontId::proportional(13.0), Color32::from_gray(230));
            let starred = st.favourites.iter().any(|f| f.eq_ignore_ascii_case(&a.rel));
            if starred {
                icons::paint(ui.painter(), Icon::Star, rect.right_center() - Vec2::new(14.0, 0.0), 15.0, ACCENT);
            }
            let mut sub = a.category.label().to_string();
            if searching && !a.dir.is_empty() {
                sub = format!("{} · {}", a.dir, sub);
            }
            if a.from_content {
                sub.push_str(&format!(" · {}", tr("mod", "мод")));
            }
            ui.painter().text(rect.min + Vec2::new(h + 6.0, 28.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.0), TEXT_DIM);
            if resp.hovered() {
                st.status_help = Some(format!("{} · {}", a.rel, tr("right-click: star it", "правый клик — в избранное")));
            }
            resp.context_menu(|ui| {
                let text = if starred { tr("Remove from favourites", "Убрать из избранного") } else { tr("Add to favourites", "Добавить в избранное") };
                if ui.button(icons::labelled(Icon::Star, text)).clicked() {
                    actions.push(Action::ToggleFavourite(a.rel.clone()));
                    ui.close();
                }
            });
            if resp.clicked() {
                actions.push(Action::UsedAsset(a.rel.clone()));
                if logic == Some(LogicMode::Paths) {
                    ed.set_path_type(a.rel.clone());
                } else if kind == Kind::Spline {
                    ed.road_type = Some(a.rel.clone());
                } else {
                    ed.object_type = Some(a.rel.clone());
                    ed.place_heading = 0.0;
                    let hint = crate::editor::asset_hint(a);
                    if !hint.is_empty() {
                        ed.toast(hint);
                    }
                }
            }
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if searching {
                // a search lists what it finds, wherever it lies
                for (i, a) in items.iter().take(400).enumerate() {
                    row(ui, a, i, st, ed);
                }
                return;
            }
            // starred and lately used ones first, among what the filter shows
            for (title, icon, list) in [(tr("Favourites", "Избранное"), Icon::Star, &st.favourites.clone()), (tr("Recently used", "Недавние"), Icon::History, &st.recent_assets.clone())] {
                let picked: Vec<&crate::library::Asset> = list.iter().filter_map(|r| items.iter().find(|a| a.rel.eq_ignore_ascii_case(r)).copied()).take(12).collect();
                if picked.is_empty() {
                    continue;
                }
                let head = icons::labelled(icon, &format!("{title}   ({})", picked.len()));
                egui::CollapsingHeader::new(head).icon(group_icon).id_salt(("cat-pinned", kind == Kind::Spline, title)).default_open(true).show(ui, |ui| {
                    for (i, a) in picked.iter().enumerate() {
                        row(ui, a, i, st, ed);
                    }
                });
            }
            // the folders as they are on disk: the first level, then what lies below it
            let mut tree: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<&crate::library::Asset>>> = std::collections::BTreeMap::new();
            for a in &items {
                let (top, rest) = match a.dir.split_once('\\') {
                    Some((t, r)) => (t.to_string(), r.to_string()),
                    None => (if a.dir.is_empty() { "—".to_string() } else { a.dir.clone() }, String::new()),
                };
                tree.entry(top).or_default().entry(rest).or_default().push(a);
            }
            let open_all = tree.len() <= 2;
            for (top, subs) in &tree {
                let n: usize = subs.values().map(|v| v.len()).sum();
                egui::CollapsingHeader::new(RichText::new(format!("{top}   ({n})")).strong()).icon(group_icon).id_salt(("cat-dir", kind == Kind::Spline, top)).default_open(open_all).show(ui, |ui| {
                    for (sub, list) in subs {
                        let mut draw = |ui: &mut egui::Ui| {
                            for (i, a) in list.iter().enumerate() {
                                row(ui, a, i, st, ed);
                            }
                        };
                        if sub.is_empty() {
                            draw(ui);
                        } else {
                            egui::CollapsingHeader::new(format!("{sub}   ({})", list.len())).icon(group_icon).id_salt(("cat-sub", kind == Kind::Spline, top, sub)).show(ui, |ui| draw(ui));
                        }
                    }
                });
            }
        });
    });
}

fn brush_grid(ui: &mut egui::Ui, ed: &mut Editor) {
    egui::Grid::new("brush").num_columns(2).spacing(Vec2::new(8.0, 6.0)).show(ui, |ui| {
        ui.label(tr("Size", "Размер"));
        ui.add(egui::Slider::new(&mut ed.brush_radius, 3.0..=150.0).suffix(" m").logarithmic(true));
        ui.end_row();
        ui.label(tr("Strength", "Сила"));
        ui.add(egui::Slider::new(&mut ed.brush_strength, 0.05..=1.0).fixed_decimals(2));
        ui.end_row();
    });
}

fn terrain_panel(ui: &mut egui::Ui, st: &mut UiState, ed: &mut Editor) {
    ui.label(RichText::new(tr("Mode", "Режим")).strong());
    for m in TerrainMode::ALL {
        let r = ui.add(egui::Button::new(icons::labelled(m.icon(), m.label())).selected(ed.terrain_mode == m).min_size(Vec2::new(ui.available_width(), 24.0)));
        if r.clicked() {
            ed.terrain_mode = m;
        }
        let h = match m {
            TerrainMode::Flatten => tr("Levels the ground to the height where you start dragging", "Выравнивает землю до высоты точки начала"),
            TerrainMode::Smooth => tr("Softens bumps and edges", "Смягчает неровности и края"),
            TerrainMode::Raise => tr("Raises the ground under the brush", "Поднимает землю под кистью"),
            TerrainMode::Lower => tr("Lowers the ground under the brush", "Опускает землю под кистью"),
        };
        help(st, &r, h);
    }
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Brush", "Кисть")).strong());
    brush_grid(ui, ed);
}

fn paint_panel(ui: &mut egui::Ui, st: &mut UiState, ed: &mut Editor, swatches: &HashMap<String, egui::TextureHandle>, wanted: &mut Vec<String>, actions: &mut Vec<Action>) {
    let Some(doc) = ed.doc.as_mut() else { return };
    ui.label(RichText::new(tr("Ground layers", "Слои земли")).strong());
    let mut remove: Option<usize> = None;
    let n = doc.global.ground_textures.len();
    Frame::new().fill(BG_FIELD).stroke(Stroke::new(1.0, BORDER)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in 0..n {
            let g = &doc.global.ground_textures[i];
            let on = ed.paint_layer == i;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click());
            ui.painter().rect_filled(rect, CornerRadius::ZERO, if on { SELECT } else if resp.hovered() { Color32::from_gray(40) } else { Color32::TRANSPARENT });
            let sw = egui::Rect::from_min_size(rect.min + Vec2::new(3.0, 3.0), Vec2::splat(32.0));
            match swatches.get(&g.texture.to_lowercase()) {
                Some(h) => {
                    egui::Image::new((h.id(), sw.size())).paint_at(ui, sw);
                }
                None => wanted.push(g.texture.clone()),
            }
            let name = std::path::Path::new(&g.texture.replace('\\', "/")).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            ui.painter().text(rect.min + Vec2::new(42.0, 12.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), Color32::from_gray(230));
            let sub = if i == 0 { tr("base ground", "основа").to_string() } else { format!("{} {}", tr("layer", "слой"), i) };
            ui.painter().text(rect.min + Vec2::new(42.0, 27.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.0), TEXT_DIM);
            if resp.hovered() {
                st.status_help = Some(g.texture.clone());
            }
            if resp.clicked() {
                ed.paint_layer = i;
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button(icons::labelled(Icon::Add, tr("Add…", "Добавить…"))).clicked() {
            st.texture_picker = Some(String::new());
        }
        if ui.add_enabled(ed.paint_layer > 0, egui::Button::new(icons::labelled(Icon::Delete, tr("Remove", "Убрать")))).clicked() {
            remove = Some(ed.paint_layer);
        }
    });
    if let Some(g) = doc.global.ground_textures.get_mut(ed.paint_layer) {
        let before = g.params[1];
        egui::Grid::new("layer").num_columns(2).show(ui, |ui| {
            ui.label(tr("Repeats per tile", "Повторов на тайл"));
            ui.add(egui::DragValue::new(&mut g.params[1]).range(1.0..=200.0).speed(0.5));
            ui.end_row();
        });
        if g.params[1] != before {
            let now = g.params[1];
            g.params[1] = before;
            let was = (doc.global.clone(), doc.order.clone());
            doc.global.ground_textures[ed.paint_layer].params[1] = now;
            ed.history.push_snapshot(tr("Edit ground texture", "Правка текстуры земли"), crate::history::Snapshot { global: Some(was), ..Default::default() });
            doc.global_dirty = true;
            doc.global_rev = crate::doc::next_rev();
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Brush", "Кисть")).strong());
    ui.horizontal(|ui| {
        ui.selectable_value(&mut ed.paint_erase, false, icons::labelled(Icon::Brush, tr("Paint", "Красить")));
        ui.selectable_value(&mut ed.paint_erase, true, icons::labelled(Icon::Eraser, tr("Erase", "Стирать")));
    });
    brush_grid(ui, ed);
    let _ = actions;
    if let Some(i) = remove {
        let Some(doc) = ed.doc.as_mut() else { return };
        ed.history.begin(tr("Remove ground texture", "Удаление текстуры земли"));
        let keys: Vec<_> = doc.tiles.keys().copied().collect();
        for k in keys {
            ed.history.touch_tile(doc, k);
        }
        ed.history.touch_global(doc);
        doc.global.ground_textures.remove(i);
        for t in doc.tiles.values_mut() {
            if i < t.masks.len() {
                t.masks.remove(i);
            }
            t.touch_masks();
        }
        doc.global_dirty = true;
        doc.global_rev = crate::doc::next_rev();
        ed.history.commit();
        ed.paint_layer = ed.paint_layer.min(doc.global.ground_textures.len().saturating_sub(1));
    }
}

// ---------------------------------------------------------------------------- right panel

pub(crate) fn prop_row(ui: &mut egui::Ui, name: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(name).color(TEXT_DIM));
    add(ui);
    ui.end_row();
}

fn properties(ui: &mut egui::Ui, st: &mut UiState, ed: &mut Editor, lib: &Library, actions: &mut Vec<Action>) {
    let Some(sel) = ed.selected else {
        map_properties(ui, st, ed, actions);
        return;
    };
    if ed.selection().len() > 1 {
        crate::ui_edit::group_properties(ui, ed, lib);
        return;
    }
    section(ui, tr("Properties", "Свойства"));
    let mut delete = false;
    let mut changed_labels: Option<Vec<String>> = None;
    let mut new_rot: Option<f64> = None;
    let mut new_lift: Option<f64> = None;
    let mut new_name: Option<String> = None;
    let mut new_pos: Option<(f64, f64)> = None;
    let mut ends = crate::ui_edit::EndsEdit::default();
    let mut spline_change: Option<(Option<String>, Option<bool>)> = None;
    let Some(doc) = ed.doc.as_ref() else { return };
    Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
        match sel {
            Sel::Object(k, id) => {
                let Some(o) = crate::selection::any_object(doc, k, id) else { return };
                let a = lib.find(&o.file);
                let name = a.map(|a| a.name.clone()).unwrap_or_else(|| o.file.rsplit('\\').next().unwrap_or("").to_string());
                ui.label(RichText::new(name).strong().color(Color32::WHITE));
                ui.label(RichText::new(&o.file).small().color(TEXT_DIM));
                ui.add_space(4.0);
                egui::Grid::new("obj").num_columns(2).spacing(Vec2::new(8.0, 5.0)).show(ui, |ui| {
                    prop_row(ui, tr("Type", "Тип"), |ui| {
                        ui.label(a.map(|a| a.category.label()).unwrap_or("—"));
                    });
                    prop_row(ui, "ID", |ui| {
                        ui.label(id.to_string());
                    });
                    prop_row(ui, tr("Tile", "Тайл"), |ui| {
                        ui.label(format!("{}, {}", k.0, k.1));
                    });
                    if let Some(p) = o.parent_id {
                        prop_row(ui, tr("Hangs on", "Висит на"), |ui| {
                            ui.label(format!("ID {p} · {} {}", tr("point", "точка"), o.attach_index));
                        });
                    }
                    if o.parent_id.is_none() { prop_row(ui, "X / Y", |ui| {
                        // (tile-local, as the map file keeps it; past the edge it goes over)
                        let (mut x, mut y) = (o.pos[0], o.pos[1]);
                        ui.horizontal(|ui| {
                            let a = ui.add(egui::DragValue::new(&mut x).speed(0.05).max_decimals(2));
                            let b = ui.add(egui::DragValue::new(&mut y).speed(0.05).max_decimals(2));
                            if a.changed() || b.changed() {
                                new_pos = Some((x, y));
                            }
                        });
                    }); }
                    prop_row(ui, tr("Direction", "Направление"), |ui| {
                        let mut h = o.rot[0];
                        if ui.add(egui::DragValue::new(&mut h).range(-360.0..=720.0).speed(1.0).suffix("°")).changed() {
                            new_rot = Some(h.rem_euclid(360.0));
                        }
                    });
                    if !a.map(|a| a.abs_height).unwrap_or(false) && o.parent_id.is_none() {
                        prop_row(ui, tr("Above ground", "Над землёй"), |ui| {
                            let mut z = o.pos[2];
                            if ui.add(egui::DragValue::new(&mut z).range(-20.0..=50.0).speed(0.05).suffix(" m")).changed() {
                                new_lift = Some(z);
                            }
                        });
                    }
                });
                let is_stop = a.map(|a| a.is_bus_stop).unwrap_or(false);
                let is_entry = a.map(|a| a.is_entry_point).unwrap_or(false);
                if is_entry {
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("Start point", "Точка старта")).strong());
                    if let Some(e) = doc.global.entry_points.iter().find(|e| e.object_id == id) {
                        let mut n = e.name.clone();
                        if ui.add(egui::TextEdit::singleline(&mut n).desired_width(f32::INFINITY)).changed() {
                            new_name = Some(n);
                        }
                    }
                }
                let mut labels = o.extra.clone();
                if is_stop || !labels.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(if is_stop { tr("Stop name", "Название остановки") } else { tr("Texts (labels)", "Тексты (labels)") }).strong());
                    if labels.is_empty() {
                        labels.push(String::new());
                    }
                    let n = if is_stop { 1 } else { labels.len() };
                    let mut any = false;
                    for l in labels.iter_mut().take(n) {
                        if ui.add(egui::TextEdit::singleline(l).desired_width(f32::INFINITY)).changed() {
                            any = true;
                        }
                    }
                    if any {
                        changed_labels = Some(labels);
                    }
                }
            }
            Sel::Spline(k, id) => {
                let Some(s) = doc.spline(k, id) else { return };
                let a = lib.find(&s.file);
                ui.label(RichText::new(a.map(|a| a.name.clone()).unwrap_or_else(|| s.file.clone())).strong().color(Color32::WHITE));
                ui.label(RichText::new(&s.file).small().color(TEXT_DIM));
                ui.add_space(4.0);
                egui::Grid::new("spl").num_columns(2).spacing(Vec2::new(8.0, 5.0)).show(ui, |ui| {
                    prop_row(ui, "ID", |ui| {
                        ui.label(id.to_string());
                    });
                    prop_row(ui, tr("Length", "Длина"), |ui| {
                        ui.label(format!("{:.1} m", s.length));
                    });
                    prop_row(ui, tr("Radius", "Радиус"), |ui| {
                        ui.label(if s.radius == 0.0 { tr("straight", "прямая").to_string() } else { format!("{:.0} m {}", s.radius.abs(), if s.radius > 0.0 { tr("right", "вправо") } else { tr("left", "влево") }) });
                    });
                    prop_row(ui, tr("Linked", "Связи"), |ui| {
                        ui.label(format!("{} / {}", if s.prev_id != 0 { tr("start", "начало") } else { "—" }, if s.next_id != 0 { tr("end", "конец") } else { "—" }));
                    });
                    prop_row(ui, tr("Mirrored", "Зеркально"), |ui| {
                        let mut m = s.mirror;
                        if check(ui, &mut m, "").changed() {
                            spline_change = Some((None, Some(m)));
                        }
                    });
                });
                ends = crate::ui_edit::spline_ends(ui, doc, k, id);
                if let Some(rt) = &ed.road_type {
                    if !rt.eq_ignore_ascii_case(&s.file) {
                        let n = lib.find(rt).map(|a| a.name.clone()).unwrap_or_else(|| rt.clone());
                        ui.add_space(4.0);
                        if ui.button(icons::labelled(Icon::Swap, &format!("{} «{}»", tr("Replace with", "Заменить на"), n))).clicked() {
                            spline_change = Some((Some(rt.clone()), None));
                        }
                    }
                }
            }
        }
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(icons::labelled(Icon::Focus, tr("Go to", "Перейти"))).clicked() {
                actions.push(Action::FocusSelection);
            }
            if ui.button(icons::labelled(Icon::Delete, tr("Delete", "Удалить"))).clicked() {
                delete = true;
            }
        });
    });
    let _ = st;
    if delete {
        ed.delete_selected();
        return;
    }
    if ed.doc.is_none() {
        return;
    }
    match sel {
        Sel::Object(k, id) => {
            crate::ui_edit::junction_section(ui, ed, k, id);
            if let Some((x, y)) = new_pos {
                let o = crate::doc::tile_origin(k);
                ed.put_object(k, id, glam::DVec2::new(o.x + x, o.y + y));
            } else if new_rot.is_some() || new_lift.is_some() || changed_labels.is_some() || new_name.is_some() {
                ed.begin_panel_step(tr("Edit object", "Изменение объекта"));
                let Some(doc) = ed.doc.as_mut() else { return };
                ed.history.touch_tile(doc, k);
                ed.history.touch_global(doc);
                if let Some(o) = crate::selection::any_object_mut(doc, k, id) {
                    if let Some(h) = new_rot {
                        o.rot[0] = h;
                    }
                    if let Some(z) = new_lift {
                        o.pos[2] = z;
                    }
                    if let Some(l) = changed_labels {
                        if o.flag < l.len() as i32 {
                            o.flag = l.len() as i32;
                        }
                        o.extra = l;
                    }
                }
                if let Some(n) = new_name {
                    if let Some(e) = doc.global.entry_points.iter_mut().find(|e| e.object_id == id) {
                        e.name = n;
                    }
                }
                if let Some(t) = doc.tiles.get_mut(&k) {
                    t.touch_content();
                }
                crate::editor::sync_entry_points(doc);
            } else {
                ed.end_panel_step_if_idle(ui.ctx());
            }
        }
        Sel::Spline(k, id) => {
            crate::ui_edit::apply_ends(ed, ui.ctx(), k, id, ends);
            crate::ui_edit::rows_section(ui, ed, lib);
            let Some(doc) = ed.doc.as_mut() else { return };
            if let Some((file, mirror)) = spline_change {
                ed.history.begin(tr("Edit road", "Изменение дороги"));
                ed.history.touch_tile(doc, k);
                if let Some(s) = doc.tiles.get_mut(&k).and_then(|t| t.tile.splines.iter_mut().find(|s| s.id == id)) {
                    if let Some(f) = file {
                        s.file = f;
                    }
                    if let Some(m) = mirror {
                        s.mirror = m;
                    }
                }
                if let Some(t) = doc.tiles.get_mut(&k) {
                    t.touch_content();
                }
                ed.history.commit();
            }
        }
    }
}

/// With nothing selected the right panel shows the map's own settings.
fn map_properties(ui: &mut egui::Ui, st: &mut UiState, ed: &mut Editor, actions: &mut Vec<Action>) {
    section(ui, tr("Map", "Карта"));
    let Some(doc) = ed.doc.as_mut() else { return };
    let before = (doc.global.friendly_name.clone(), doc.global.description.clone(), doc.global.left_hand_traffic);
    Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
        egui::Grid::new("map").num_columns(2).spacing(Vec2::new(8.0, 6.0)).show(ui, |ui| {
            prop_row(ui, tr("Name", "Название"), |ui| {
                ui.add(egui::TextEdit::singleline(&mut doc.global.friendly_name).desired_width(f32::INFINITY));
            });
            prop_row(ui, tr("Folder", "Папка"), |ui| {
                ui.label(doc.dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default());
            });
            prop_row(ui, tr("Traffic", "Движение"), |ui| {
                check(ui, &mut doc.global.left_hand_traffic, tr("left-hand", "левостороннее"));
            });
            prop_row(ui, tr("Start points", "Точки старта"), |ui| {
                ui.label(doc.global.entry_points.len().to_string());
            });
        });
        ui.add_space(4.0);
        crate::descriptions::editor(ui, doc, &mut ed.history);
        if doc.global.entry_points.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(tr("No start point yet: place one (Objects > Bus stops > Entrypoint) so the bus has somewhere to appear in openOMSI.", "Нет точки старта: поставьте её (Объекты > Остановки > Entrypoint), чтобы автобусу было где появиться в openOMSI.")).small().color(Color32::from_rgb(230, 170, 100)));
        }
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Picture", "Картинка")).color(TEXT_DIM));
        let path = doc.dir.join("picture.jpg");
        let path = if path.is_file() { path } else { doc.source.as_ref().map(|s| s.join("picture.jpg")).unwrap_or(path) };
        let time = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if st.map_picture.as_ref().map(|(p, t, _)| *p != path || *t != time).unwrap_or(true) {
            let tex = image::open(&path).ok().map(|img| {
                let img = img.to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                ui.ctx().load_texture("map picture", ci, egui::TextureOptions::LINEAR)
            });
            st.map_picture = Some((path, time, tex));
        }
        match st.map_picture.as_ref().and_then(|(_, _, t)| t.as_ref()) {
            Some(t) => {
                let w = ui.available_width();
                ui.add(egui::Image::new((t.id(), Vec2::new(w, w * 280.0 / 370.0))));
            }
            None => {
                ui.label(RichText::new(tr("None yet: openOMSI shows it where maps are chosen.", "Пока нет: openOMSI показывает её при выборе карты.")).small().color(TEXT_DIM));
            }
        }
        let r = ui.button(icons::labelled(Icon::Visible, tr("Take it from this view", "Снять с этого вида")));
        help(st, &r, tr("The middle of the 3D view, as it is now, becomes the map's picture.", "Середина 3D-вида, как он сейчас, станет картинкой карты."));
        if r.clicked() {
            actions.push(Action::MapPicture);
        }
        let r = ui.button(icons::labelled(Icon::Flag, tr("Open the map with this view", "Открывать карту с этого вида")));
        help(st, &r, tr("The camera as it is now is where the map opens (here and in OMSI 2's editor).", "Карта будет открываться с камерой, как сейчас (здесь и в редакторе OMSI 2)."));
        if r.clicked() {
            actions.push(Action::SetMapCamera);
        }
        ui.add_space(6.0);
        ui.label(RichText::new(format!("{}:\n{}", tr("Saved to", "Сохраняется в"), doc.dir.display())).small().color(TEXT_DIM));
    });
    let after = (doc.global.friendly_name.clone(), doc.global.description.clone(), doc.global.left_hand_traffic);
    if before != after {
        doc.global_dirty = true;
        // (a step of its own: undoing an earlier edit of the global settings would else take
        // this back too)
        let mut g = doc.global.clone();
        (g.friendly_name, g.description, g.left_hand_traffic) = before;
        ed.history.push_snapshot(tr("Edit map", "Правка карты"), crate::history::Snapshot { global: Some((g, doc.order.clone())), ..Default::default() });
    }
}

// ---------------------------------------------------------------------------- dialogs

fn texture_picker(ctx: &egui::Context, st: &mut UiState, files: &[String], swatches: &HashMap<String, egui::TextureHandle>, wanted: &mut Vec<String>, actions: &mut Vec<Action>) {
    let Some(search) = &mut st.texture_picker else { return };
    let mut open = true;
    let mut chosen = None;
    egui::Window::new(tr("Add ground texture", "Добавить текстуру земли")).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.set_width(470.0);
        ui.horizontal(|ui| {
            ui.label(tr("Search", "Поиск"));
            ui.add(egui::TextEdit::singleline(search).desired_width(f32::INFINITY));
        });
        let q = search.to_lowercase();
        let list: Vec<&String> = files.iter().filter(|f| f.to_lowercase().contains(&q)).collect();
        let cols = 5;
        Frame::new().fill(BG_FIELD).stroke(Stroke::new(1.0, BORDER)).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, false]).show_rows(ui, 94.0, list.len().div_ceil(cols), |ui, range| {
                for r in range {
                    ui.horizontal(|ui| {
                        for f in list.iter().skip(r * cols).take(cols) {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(86.0, 90.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, CornerRadius::same(2), SELECT);
                            }
                            let img = egui::Rect::from_min_size(rect.min + Vec2::new(5.0, 3.0), Vec2::splat(76.0));
                            match swatches.get(&f.to_lowercase()) {
                                Some(h) => {
                                    egui::Image::new((h.id(), img.size())).paint_at(ui, img);
                                }
                                None => wanted.push((*f).clone()),
                            }
                            let name = f.rsplit('\\').next().unwrap_or(f);
                            ui.painter().text(egui::pos2(rect.center().x, rect.max.y - 5.0), egui::Align2::CENTER_CENTER, name.chars().take(14).collect::<String>(), egui::FontId::proportional(10.0), Color32::from_gray(200));
                            if resp.clicked() {
                                chosen = Some((*f).clone());
                            }
                        }
                    });
                }
            });
        });
    });
    if let Some(c) = chosen {
        actions.push(Action::AddGroundTexture(c));
        open = false;
    }
    if !open {
        st.texture_picker = None;
    }
}

fn controls(ctx: &egui::Context, st: &mut UiState) {
    let mut open = st.help_open;
    egui::Window::new(tr("Controls", "Управление")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let rows: &[(&str, &str)] = &[
            (tr("Mouse", "Мышь"), ""),
            (tr("Right button held", "Правая кнопка зажата"), tr("look round; W A S D fly, Q / E down / up, wheel: speed", "обзор; W A S D — полёт, Q / E — вниз / вверх, колесо — скорость")),
            (tr("Middle drag", "Средняя кнопка"), tr("pan", "сдвиг вида")),
            (tr("Alt + left drag", "Alt + левая кнопка"), tr("orbit round the point under the cursor", "орбита вокруг точки под курсором")),
            (tr("Wheel", "Колесо"), tr("move towards the cursor", "приближение к курсору")),
            (tr("Left drag on empty ground", "Левая кнопка по пустой земле"), tr("move the map (Select tool)", "двигать карту (инструмент «Выбор»)")),
            (tr("Trackpad", "Трекпад"), ""),
            (tr("Two fingers", "Два пальца"), tr("pan", "сдвиг вида")),
            (tr("Pinch / twist", "Щипок / поворот"), tr("zoom / turn", "зум / поворот")),
            (tr("Option + two fingers or drag", "Option + два пальца или drag"), tr("orbit round the point under the cursor", "орбита вокруг точки под курсором")),
            (tr("Keyboard", "Клавиатура"), ""),
            (tr("W A S D / arrow keys", "W A S D / стрелки"), tr("move over the map (Shift: faster)", "двигаться по карте (Shift — быстрее)")),
            ("Q / E", tr("turn the camera", "поворачивать камеру")),
            ("1 – 7", tr("tools", "инструменты")),
            ("R / Shift+R, Ctrl+wheel", tr("turn the object", "повернуть объект")),
            ("Del", tr("delete the selection", "удалить выбранное")),
            ("Shift+click", tr("add to / take from the selection", "добавить к выделению / убрать из него")),
            (tr("Shift / Ctrl + drag on empty ground", "Shift / Ctrl + тянуть по пустой земле"), tr("select with a box (Shift adds)", "выделить рамкой (Shift — добавить)")),
            ("Ctrl+C / X / V", tr("copy / cut / paste at the cursor", "копировать / вырезать / вставить у курсора")),
            ("Ctrl+D / Ctrl+A", tr("duplicate / select all", "дублировать / выбрать всё")),
            ("U", tr("ruler: click two points (distance, height, gradient)", "линейка: клик по двум точкам (расстояние, высота, уклон)")),
            ("F", tr("go to the selection", "перейти к выбранному")),
            ("Esc", tr("stop / deselect", "отмена / снять выбор")),
            ("[ ]  Shift+wheel", tr("brush size", "размер кисти")),
            ("G", tr("tile grid on / off", "сетка тайлов")),
            ("M", tr("overview map on / off", "обзорная карта")),
            ("Ctrl+Z / Ctrl+Shift+Z", tr("undo / redo", "отменить / повторить")),
            ("Ctrl+S", tr("save", "сохранить")),
            ("F5", tr("test drive in openOMSI", "тест-драйв в openOMSI")),
        ];
        egui::Grid::new("keys").striped(true).num_columns(2).show(ui, |ui| {
            for (k, v) in rows {
                if v.is_empty() {
                    ui.label(RichText::new(*k).strong().color(ACCENT));
                } else {
                    ui.label(RichText::new(*k).color(Color32::from_gray(230)));
                }
                ui.label(*v);
                ui.end_row();
            }
        });
    });
    st.help_open = open;
}

/// Ask before something unsaved is lost.
pub fn confirm_unsaved(ctx: &egui::Context, st: &mut UiState, actions: &mut Vec<Action>) {
    if st.confirm.is_none() {
        return;
    }
    let mut decided: Option<bool> = None;
    egui::Window::new(tr("Unsaved changes", "Несохранённые изменения")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.set_width(330.0);
        ui.horizontal(|ui| {
            ui.label(Icon::Info.text(28.0).color(ACCENT));
            ui.label(tr("The map has changes that are not saved. Save them first?", "На карте есть несохранённые изменения. Сохранить их?"));
        });
        ui.separator();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(tr("Cancel", "Отмена")).clicked() {
                decided = Some(false);
            }
            if ui.button(tr("Don't save", "Не сохранять")).clicked() {
                decided = Some(true);
            }
            if ui.button(icons::labelled(Icon::Save, tr("Save", "Сохранить"))).clicked() {
                actions.push(Action::Save);
                decided = Some(true);
            }
        });
    });
    match decided {
        Some(true) => {
            if let Some(a) = st.confirm.take() {
                actions.push(Action::Force(Box::new(a)));
            }
        }
        Some(false) => st.confirm = None,
        None => {}
    }
}
