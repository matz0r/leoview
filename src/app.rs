use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, Align, Align2, Color32, ColorImage, CornerRadius, CursorIcon, FontId, Frame, Key, Layout,
    Margin, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui,
    UiBuilder, Vec2, pos2, vec2,
};

use crate::layout::justify;
use crate::loader::{Kind, Loader, THUMB_MAX};

pub const ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");
/// Logo of xpaint, shown on the "Edit in xpaint" button.
#[cfg(target_os = "linux")]
const XPAINT_ICON_PNG: &[u8] = include_bytes!("../assets/xpaint-icon.png");

// Toolbar icons: white line drawings (shared with xpaint), tinted when painted.
const ABOUT_ICON: egui::ImageSource = egui::include_image!("../assets/icons/about.svg");
const DELETE_ICON: egui::ImageSource = egui::include_image!("../assets/icons/delete.svg");
const SETTINGS_ICON: egui::ImageSource = egui::include_image!("../assets/icons/settings.svg");
#[cfg(any(target_os = "linux", windows))]
const INSTALL_ICON: egui::ImageSource = egui::include_image!("../assets/icons/install.svg");
const THEME_SYSTEM_ICON: egui::ImageSource =
    egui::include_image!("../assets/icons/theme-system.svg");
const THEME_DARK_ICON: egui::ImageSource = egui::include_image!("../assets/icons/theme-dark.svg");
const THEME_LIGHT_ICON: egui::ImageSource = egui::include_image!("../assets/icons/theme-light.svg");

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "ico", "tga", "qoi", "pnm", "pbm",
    "pgm", "ppm",
];
// Sizes at the Normal icon size; the icon size setting scales them (see `icon_scale`).
const TOOLBAR_HEIGHT: f32 = 42.0;
const FILMSTRIP_HEIGHT: f32 = 92.0;
const GRID_SPACING: f32 = 8.0;
/// Assumed aspect ratio while an image's real one is still being read.
const DEFAULT_ASPECT: f32 = 1.5;
const THUMB_RADIUS: u8 = 6;
const ICON_BUTTON: Vec2 = vec2(30.0, 26.0);
/// Storage key of the icon size setting (the same as xpaint's).
const ICON_SIZE_KEY: &str = "icon_size";
/// Touchpad scroll distance (points) that counts as one wheel notch.
const SCROLL_STEP_POINTS: f32 = 60.0;
/// How many images before/after the selected one are preloaded at full size.
const PRELOAD_RADIUS: usize = 1;

/// How large the toolbar icons are drawn, together with their buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum IconSize {
    #[default]
    Normal,
    Large,
    Larger,
}

impl IconSize {
    const ALL: [IconSize; 3] = [IconSize::Normal, IconSize::Large, IconSize::Larger];

    fn name(self) -> &'static str {
        match self {
            IconSize::Normal => "Normal",
            IconSize::Large => "Large",
            IconSize::Larger => "Larger",
        }
    }

    fn scale(self) -> f32 {
        match self {
            IconSize::Normal => 1.0,
            IconSize::Large => 1.25,
            IconSize::Larger => 1.5,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|size| size.name() == name)
    }
}

enum Tex {
    Loading,
    Ready(TextureHandle),
    Failed(String),
}

pub struct LeoView {
    loader: Loader,
    logo: TextureHandle,
    #[cfg(target_os = "linux")]
    xpaint_logo: TextureHandle,
    current_dir: PathBuf,
    subdirs: Vec<PathBuf>,
    /// Drive roots (Windows only, empty elsewhere).
    drives: Vec<PathBuf>,
    images: Vec<PathBuf>,
    /// `Some` = single image view, `None` = directory (thumbnail grid) view.
    selected: Option<usize>,
    thumb_size: f32,
    thumbs: HashMap<PathBuf, Tex>,
    fulls: HashMap<PathBuf, Tex>,
    dir_error: Option<String>,
    show_about: bool,
    /// Folder sidebar visible (toggled by the toolbar button).
    show_sidebar: bool,
    /// Show the Install button (running from a download, not the installed copy).
    offer_install: bool,
    /// Error window: title and message (failed Install or Update).
    error: Option<(&'static str, String)>,
    /// Running from the installed copy: the only one that offers updates.
    #[cfg(any(target_os = "linux", windows))]
    update_eligible: bool,
    #[cfg(any(target_os = "linux", windows))]
    update: crate::update::Settings,
    #[cfg(any(target_os = "linux", windows))]
    updating: bool,
    #[cfg(any(target_os = "linux", windows))]
    update_rx: Option<std::sync::mpsc::Receiver<crate::update::Msg>>,
    scroll_accum: f32,
    /// Horizontal center of the image area last frame, to align the toolbar arrows.
    viewer_center_x: Option<f32>,
    /// Aspect ratio (width / height) per image, 0.0 while unknown.
    aspects: Vec<f32>,
    icon_size: IconSize,
    /// Dev aid: `LEOVIEW_SCREENSHOT=out.png` saves the app's own frame and quits.
    screenshot_to: Option<PathBuf>,
}

impl LeoView {
    pub fn new(cc: &eframe::CreationContext<'_>, start: Option<PathBuf>) -> Self {
        setup_style(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let logo = load_logo(&cc.egui_ctx, "logo", ICON_PNG);
        #[cfg(target_os = "linux")]
        let xpaint_logo = load_logo(&cc.egui_ctx, "xpaint-logo", XPAINT_ICON_PNG);

        #[cfg(any(target_os = "linux", windows))]
        let update_eligible = crate::update::eligible();
        // A downloaded copy neither reads nor changes the update settings.
        #[cfg(any(target_os = "linux", windows))]
        let update = if update_eligible {
            crate::update::Settings::load()
        } else {
            Default::default()
        };

        let mut app = Self {
            loader: Loader::new(&cc.egui_ctx),
            logo,
            #[cfg(target_os = "linux")]
            xpaint_logo,
            current_dir: PathBuf::new(),
            subdirs: Vec::new(),
            drives: list_drives(),
            images: Vec::new(),
            selected: None,
            thumb_size: 160.0,
            thumbs: HashMap::new(),
            fulls: HashMap::new(),
            dir_error: None,
            show_about: false,
            show_sidebar: true,
            #[cfg(any(target_os = "linux", windows))]
            offer_install: crate::install::available(),
            #[cfg(not(any(target_os = "linux", windows)))]
            offer_install: false,
            error: None,
            // Checked at start, at most once per day.
            #[cfg(any(target_os = "linux", windows))]
            update_rx: (update_eligible
                && update.check
                && crate::update::due(update.last_check, crate::update::now()))
            .then(|| crate::update::check(&cc.egui_ctx)),
            #[cfg(any(target_os = "linux", windows))]
            update_eligible,
            #[cfg(any(target_os = "linux", windows))]
            update,
            #[cfg(any(target_os = "linux", windows))]
            updating: false,
            scroll_accum: 0.0,
            viewer_center_x: None,
            aspects: Vec::new(),
            icon_size: cc
                .storage
                .and_then(|s| s.get_string(ICON_SIZE_KEY))
                .and_then(|name| IconSize::from_name(&name))
                .unwrap_or_default(),
            screenshot_to: std::env::var_os("LEOVIEW_SCREENSHOT").map(PathBuf::from),
        };

        let start = start
            // `dunce` avoids Windows `\\?\` verbatim paths, which would show up in the UI.
            .and_then(|p| dunce::canonicalize(p).ok())
            .unwrap_or_else(default_dir);
        if start.is_file() {
            let dir = start
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(default_dir);
            app.open_dir(dir);
            let idx = app.images.iter().position(|p| *p == start);
            app.select(idx);
        } else {
            app.open_dir(start);
        }
        app
    }

    fn open_dir(&mut self, dir: PathBuf) {
        self.subdirs.clear();
        self.images.clear();
        self.thumbs.clear();
        self.fulls.clear();
        self.selected = None;
        self.dir_error = None;
        self.loader.new_generation(Kind::Thumb);
        self.loader.new_generation(Kind::Full);

        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if is_hidden(&entry) {
                        continue;
                    }
                    let path = entry.path();
                    if path.is_dir() {
                        self.subdirs.push(path);
                    } else if is_image(&path) {
                        self.images.push(path);
                    }
                }
                self.subdirs.sort_by_key(|p| sort_key(p));
                self.images.sort_by_key(|p| sort_key(p));
            }
            Err(e) => self.dir_error = Some(format!("Cannot read {}\n{e}", dir.display())),
        }
        self.aspects = vec![0.0; self.images.len()];
        self.loader.scan_aspects(self.images.clone());
        self.current_dir = dir;
    }

    fn select(&mut self, idx: Option<usize>) {
        self.selected = idx.filter(|&i| i < self.images.len());
        self.scroll_accum = 0.0;
        let Some(idx) = self.selected else {
            self.fulls.clear();
            return;
        };

        // Drop stale requests and images that are no longer near the selection.
        self.loader.new_generation(Kind::Full);
        let lo = idx.saturating_sub(PRELOAD_RADIUS);
        let hi = (idx + PRELOAD_RADIUS).min(self.images.len() - 1);
        let wanted = &self.images[lo..=hi];
        self.fulls
            .retain(|path, tex| !matches!(tex, Tex::Loading) && wanted.contains(path));

        // Selected image first, then its neighbours outward.
        let mut order = vec![idx];
        for d in 1..=PRELOAD_RADIUS {
            order.extend(idx.checked_add(d).filter(|&i| i <= hi));
            order.extend(idx.checked_sub(d).filter(|&i| i >= lo));
        }
        for i in order {
            let path = self.images[i].clone();
            if !self.fulls.contains_key(&path) {
                self.fulls.insert(path.clone(), Tex::Loading);
                self.loader.request(Kind::Full, path);
            }
        }
    }

    /// Move to the OS trash; keep the selection intact if that fails.
    fn delete_selected(&mut self) {
        let Some(index) = self.selected else { return };
        let path = self.images[index].clone();
        if let Err(error) = trash::delete(&path) {
            self.error = Some((
                "Can't move image to recycle bin",
                format!("{}\n{error}", path.display()),
            ));
            return;
        }
        self.images.remove(index);
        self.aspects.remove(index);
        self.thumbs.remove(&path);
        self.fulls.remove(&path);
        // Aspect results use indices, so invalidate results from before the removal.
        self.loader.scan_aspects(self.images.clone());
        self.select(if self.images.is_empty() {
            None
        } else {
            Some(index.min(self.images.len() - 1))
        });
    }

    /// Moves the selection by `delta` images, clamped to the list.
    fn step(&mut self, delta: isize) {
        if let Some(i) = self.selected {
            let last = self.images.len().saturating_sub(1);
            let next = i.saturating_add_signed(delta).min(last);
            if next != i {
                self.select(Some(next));
            }
        }
    }

    /// Returns the thumbnail texture for an image, requesting it if needed.
    fn thumb(&mut self, idx: usize) -> Option<&Tex> {
        let path = self.images.get(idx)?;
        if !self.thumbs.contains_key(path) {
            self.thumbs.insert(path.clone(), Tex::Loading);
            self.loader.request(Kind::Thumb, path.clone());
        }
        self.thumbs.get(path)
    }

    fn poll_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.loader.try_recv_aspect() {
            if result.generation == self.loader.aspect_generation()
                && let Some(slot) = self.aspects.get_mut(result.index)
            {
                *slot = result.aspect.unwrap_or(1.0);
            }
        }
        while let Some(loaded) = self.loader.try_recv() {
            if loaded.generation != self.loader.generation(loaded.kind) {
                continue;
            }
            let (map, options) = match loaded.kind {
                Kind::Thumb => (&mut self.thumbs, TextureOptions::LINEAR),
                Kind::Full => (
                    &mut self.fulls,
                    TextureOptions {
                        mipmap_mode: Some(egui::TextureFilter::Linear),
                        ..TextureOptions::LINEAR
                    },
                ),
            };
            // Ignore results for entries that were evicted in the meantime.
            let Some(slot) = map.get_mut(&loaded.path) else {
                continue;
            };
            *slot = match loaded.image {
                Ok(image) => {
                    let name = loaded.path.to_string_lossy();
                    Tex::Ready(ctx.load_texture(name, image, options))
                }
                Err(e) => Tex::Failed(e),
            };
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // Don't steal keys from a focused widget (e.g. the slider).
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        if !self.show_about
            && self.error.is_none()
            && ctx.input(|i| {
                i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Delete,
                            pressed: true,
                            repeat: false,
                            ..
                        }
                    )
                })
            })
        {
            self.delete_selected();
            return;
        }
        let pressed = |key| ctx.input(|i| i.key_pressed(key));
        match self.selected {
            Some(_) => {
                if pressed(Key::ArrowRight) || pressed(Key::Space) {
                    self.step(1);
                } else if pressed(Key::ArrowLeft) {
                    self.step(-1);
                } else if pressed(Key::Home) {
                    self.select(Some(0));
                } else if pressed(Key::End) {
                    self.select(Some(self.images.len() - 1));
                } else if pressed(Key::Escape) || pressed(Key::Backspace) {
                    self.select(None);
                }
            }
            None => {
                if pressed(Key::Backspace)
                    && let Some(parent) = self.current_dir.parent()
                {
                    self.open_dir(parent.to_path_buf());
                }
            }
        }
    }

    /// Steps through images with the mouse wheel while the pointer is over `rect`.
    fn scroll_to_step(&mut self, ui: &Ui, rect: Rect) {
        if !ui.rect_contains_pointer(rect) {
            return;
        }
        // Raw wheel events, not egui's smoothed scroll delta: that spreads a
        // notch over several frames and is in points (40 per notch), which
        // made single notches get lost. Counted in notches (lines).
        let notches: f32 = ui.input(|i| {
            i.events
                .iter()
                .map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, .. } => {
                        let d = delta.x + delta.y;
                        match unit {
                            egui::MouseWheelUnit::Point => d / SCROLL_STEP_POINTS,
                            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => d,
                        }
                    }
                    _ => 0.0,
                })
                .sum()
        });
        if notches == 0.0 {
            return;
        }
        // Reversing direction drops the leftover from the other way.
        if notches.signum() != self.scroll_accum.signum() {
            self.scroll_accum = 0.0;
        }
        self.scroll_accum += notches;
        // Positive = content moves down = wheel up = previous image.
        let steps = self.scroll_accum.trunc();
        if steps != 0.0 {
            let leftover = self.scroll_accum - steps;
            // Each notch is one image, also for fast spins; the leftover
            // fraction (high-resolution wheels, touchpads) carries over.
            // Set after stepping: selecting an image resets the accumulator.
            self.step(-(steps as isize));
            self.scroll_accum = leftover;
        }
    }

    fn take_screenshot_when_ready(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.screenshot_to else {
            return;
        };
        let loading = self.thumbs.values().chain(self.fulls.values());
        let busy = loading.into_iter().any(|t| matches!(t, Tex::Loading));
        if (busy && ctx.input(|i| i.time) < 5.0) || ctx.input(|i| i.time) < 1.0 {
            ctx.request_repaint();
            return;
        }
        let path = path.clone();
        self.screenshot_to = None;
        let callback = egui::ScreenshotCallback::new(move |image: Arc<ColorImage>| {
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            let [w, h] = image.size;
            if let Err(e) =
                image::save_buffer(&path, &bytes, w as u32, h as u32, image::ColorType::Rgba8)
            {
                eprintln!("screenshot failed: {e}");
            }
            std::process::exit(0);
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
        ctx.request_repaint();
    }

    // ---- UI sections ----

    fn toolbar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let row = |min_x: f32, max_x: f32| {
            Rect::from_min_max(
                pos2(min_x, rect.top()),
                pos2(max_x.max(min_x), rect.bottom()),
            )
        };

        // Image view: prev/next centered over the image, "back to folder" left of them.
        let mut left_end = rect.right();
        let mut right_start = rect.left();
        if let Some(i) = self.selected {
            let counter = format!("{} / {}", i + 1, self.images.len());
            let counter_width = 64.0_f32.max(
                ui.painter()
                    .layout_no_wrap(counter.clone(), FontId::proportional(14.0), Color32::WHITE)
                    .size()
                    .x
                    + 8.0,
            );
            let gap = ui.spacing().item_spacing.x;
            let button = ICON_BUTTON * icon_scale(ui);
            let group_width = 2.0 * button.x + counter_width + 2.0 * gap;
            let center_x = self.viewer_center_x.unwrap_or(rect.center().x);
            let group = Rect::from_center_size(
                pos2(center_x, rect.center().y),
                vec2(group_width, button.y),
            );

            let prev = Rect::from_min_size(group.min, button);
            let next = Rect::from_min_size(pos2(group.right() - button.x, group.top()), button);
            let counter_rect = Rect::from_min_max(
                pos2(prev.right() + gap, group.top()),
                pos2(next.left() - gap, group.bottom()),
            );
            if put_enabled(ui, prev, i > 0, icon_button_widget(ui, "◀"))
                .on_hover_text("Previous (←)")
                .clicked()
            {
                self.step(-1);
            }
            ui.put(
                counter_rect,
                egui::Label::new(RichText::new(counter).weak()),
            );
            if put_enabled(
                ui,
                next,
                i + 1 < self.images.len(),
                icon_button_widget(ui, "▶"),
            )
            .on_hover_text("Next (→)")
            .clicked()
            {
                self.step(1);
            }

            let folder_area = row(rect.left(), group.left() - 2.0 * gap);
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(folder_area)
                    .layout(Layout::right_to_left(Align::Center)),
                |ui| {
                    if ui
                        .button("⬅  Folder")
                        .on_hover_text("Back to folder (Esc)")
                        .clicked()
                    {
                        self.select(None);
                    }
                    left_end = ui.min_rect().left() - 2.0 * gap;
                },
            );
            right_start = group.right() + 2.0 * gap;
        }

        // Left: branding, plus the size slider in the overview.
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(row(rect.left(), left_end))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.add(
                    egui::Image::from_texture(&self.logo)
                        .fit_to_exact_size(Vec2::splat(24.0 * icon_scale(ui)))
                        .corner_radius(5.0 * icon_scale(ui)),
                );
                ui.label(RichText::new("LeoView").strong().size(15.0));
                ui.separator();
                self.sidebar_toggle(ui);
                if self.selected.is_none() {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.label(RichText::new("Preview size").weak());
                    ui.add(
                        egui::Slider::new(&mut self.thumb_size, 64.0..=THUMB_MAX as f32)
                            .suffix(" px")
                            .integer(),
                    );
                }
            },
        );

        // Right: info, theme, About.
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(row(right_start, rect.right()))
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                if svg_button(ui, ABOUT_ICON, "About LeoView").clicked() {
                    self.show_about = true;
                }
                let settings = svg_button(ui, SETTINGS_ICON, "Settings");
                egui::Popup::menu(&settings)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| self.settings_menu(ui));
                #[cfg(any(target_os = "linux", windows))]
                if self.offer_install {
                    let tooltip = if crate::install::has_installed_copy() {
                        "Replace the installed copy with this version"
                    } else {
                        "Install in your user folder and application menu, \
                         so you can delete this download"
                    };
                    if ui.button("⬇  Install").on_hover_text(tooltip).clicked() {
                        self.install(ui.ctx());
                    }
                }
                #[cfg(any(target_os = "linux", windows))]
                if self.updating {
                    ui.label(RichText::new("Updating…").weak());
                    ui.spinner();
                } else if let Some(version) = self.update.offered().cloned()
                    && ui
                        .add(egui::Button::new(format!("⬆  Update to {version}")).selected(true))
                        .on_hover_text("Download it from GitHub, install it and restart")
                        .clicked()
                {
                    self.start_update(ui.ctx(), version);
                }
                ui.separator();
                #[cfg(target_os = "linux")]
                if let Some(i) = self.selected
                    && let Some(binary) = crate::xpaint::installed()
                {
                    let scale = icon_scale(ui);
                    let logo = egui::Image::from_texture(&self.xpaint_logo)
                        .fit_to_exact_size(Vec2::splat(20.0 * scale))
                        .corner_radius(4.0 * scale);
                    if ui
                        .add(egui::Button::image(logo).min_size(ICON_BUTTON * scale))
                        .on_hover_text("Edit in xpaint")
                        .clicked()
                        && let Err(e) = crate::xpaint::open(&binary, &self.images[i])
                    {
                        self.error = Some(("Can't open xpaint", e));
                    }
                    ui.separator();
                }
                if self.selected.is_some()
                    && svg_button(ui, DELETE_ICON, "Move to recycle bin (Del)").clicked()
                {
                    self.delete_selected();
                }
                theme_buttons(ui);
                ui.separator();

                let weak = ui.visuals().weak_text_color();
                let strong = ui.visuals().strong_text_color();
                let font = FontId::proportional(14.0);
                let mut job = LayoutJob::default();
                match self.selected {
                    Some(i) => {
                        if let Some(Tex::Ready(tex)) = self.fulls.get(&self.images[i]) {
                            let [w, h] = tex.size();
                            job.append(
                                &format!("{w} × {h}"),
                                0.0,
                                TextFormat::simple(font.clone(), weak),
                            );
                        }
                        job.append(
                            &file_name(&self.images[i]),
                            if job.is_empty() { 0.0 } else { 12.0 },
                            TextFormat::simple(font, strong),
                        );
                    }
                    None => job.append(
                        &count_label(self.images.len(), "image"),
                        0.0,
                        TextFormat::simple(font, weak),
                    ),
                }
                ui.add(egui::Label::new(job).truncate());
            },
        );
    }

    /// Button that shows/hides the folder sidebar, drawn as a window with a
    /// left panel (filled while the sidebar is shown).
    fn sidebar_toggle(&mut self, ui: &mut Ui) {
        let response = ui
            .add(
                egui::Button::new("")
                    .min_size(ICON_BUTTON * icon_scale(ui))
                    .selected(self.show_sidebar),
            )
            .on_hover_text(if self.show_sidebar {
                "Hide folders"
            } else {
                "Show folders"
            });
        if response.clicked() {
            self.show_sidebar = !self.show_sidebar;
        }
        let color = if self.show_sidebar {
            ui.visuals().selection.stroke.color
        } else {
            ui.style().interact(&response).fg_stroke.color
        };
        let scale = icon_scale(ui);
        let stroke = Stroke::new(1.3 * scale, color);
        let frame = Rect::from_center_size(response.rect.center(), vec2(16.0, 13.0) * scale);
        let painter = ui.painter();
        painter.rect_stroke(frame, 2.0 * scale, stroke, StrokeKind::Inside);
        let strip = Rect::from_min_max(frame.min, pos2(frame.left() + 6.0 * scale, frame.bottom()));
        if self.show_sidebar {
            painter.rect_filled(
                strip,
                CornerRadius {
                    nw: (2.0 * scale) as u8,
                    sw: (2.0 * scale) as u8,
                    ne: 0,
                    se: 0,
                },
                color,
            );
        } else {
            painter.vline(strip.right(), frame.y_range(), stroke);
        }
    }

    fn file_browser(&mut self, ui: &mut Ui) {
        let mut target = None;

        ui.horizontal(|ui| {
            ui.label(RichText::new("FOLDERS").small().strong().weak());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let parent = self.current_dir.parent();
                if icon_button(ui, "⬆", parent.is_some(), "Parent folder (Backspace)").clicked() {
                    target = parent.map(Path::to_path_buf);
                }
                if icon_button(ui, "🏠", true, "Home").clicked() {
                    target = Some(default_dir());
                    self.drives = list_drives();
                }
            });
        });
        if !self.drives.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for drive in &self.drives {
                    let label = drive.display().to_string();
                    let current = self.current_dir.starts_with(drive);
                    if ui
                        .selectable_label(current, label.trim_end_matches('\\'))
                        .on_hover_text(format!("Open drive {label}"))
                        .clicked()
                    {
                        target = Some(drive.clone());
                    }
                }
            });
        }
        ui.add_space(2.0);
        ui.label(RichText::new(file_name(&self.current_dir)).strong())
            .on_hover_text(self.current_dir.display().to_string());
        ui.add(
            egui::Label::new(
                RichText::new(self.current_dir.display().to_string())
                    .small()
                    .weak(),
            )
            .wrap(),
        );
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(2.0);

        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let row = vec2(ui.available_width(), 26.0);
                for dir in &self.subdirs {
                    let button = egui::Button::selectable(false, format!("📁  {}", file_name(dir)))
                        .truncate()
                        .min_size(row);
                    if ui
                        .add(button)
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    {
                        target = Some(dir.clone());
                    }
                }
                if self.subdirs.is_empty() {
                    ui.add_space(4.0);
                    ui.weak("No subfolders");
                }
            });

        if let Some(dir) = target {
            self.open_dir(dir);
        }
    }

    fn grid_view(&mut self, ui: &mut Ui) {
        if let Some(err) = &self.dir_error {
            let color = ui.visuals().error_fg_color;
            empty_state(ui, "⚠", err, color);
            return;
        }
        if self.images.is_empty() {
            let color = ui.visuals().weak_text_color();
            empty_state(ui, "🖼", "No images in this folder", color);
            return;
        }

        let target_height = self.thumb_size.round();
        // Unknown aspect ratios (header not read yet) get a typical photo shape.
        let aspects: Vec<f32> = self
            .aspects
            .iter()
            .map(|&a| if a > 0.0 { a } else { DEFAULT_ASPECT })
            .collect();

        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show_viewport(ui, |ui, viewport| {
                let width = ui.available_width();
                let (rows, total_height) = justify(&aspects, width, target_height, GRID_SPACING);
                ui.set_min_size(vec2(width, total_height));
                let origin = ui.max_rect().min;

                let visible = rows
                    .iter()
                    .filter(|r| r.y + r.height >= viewport.min.y && r.y <= viewport.max.y);
                for row in visible {
                    let mut x = 0.0;
                    for (idx, aspect) in aspects.iter().enumerate().take(row.end).skip(row.start) {
                        let w = aspect * row.height;
                        let rect =
                            Rect::from_min_size(origin + vec2(x, row.y), vec2(w, row.height));
                        x += w + GRID_SPACING;
                        if self.grid_item(ui, idx, rect).clicked() {
                            clicked = Some(idx);
                        }
                    }
                }
            });

        if clicked.is_some() {
            self.select(clicked);
        }
    }

    fn grid_item(&mut self, ui: &Ui, idx: usize, rect: Rect) -> egui::Response {
        let response = ui
            .interact(rect, ui.scope_id().with(("grid", idx)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        self.paint_thumb(ui, idx, rect);
        if !response.hovered() {
            return response;
        }

        // Highlight and show the file name in a bar along the bottom edge.
        let painter = ui.painter();
        let radius = CornerRadius::same(THUMB_RADIUS);
        let bar_height = 24.0_f32.min(rect.height());
        let bar = Rect::from_min_max(pos2(rect.left(), rect.bottom() - bar_height), rect.max);
        painter.rect_filled(
            bar,
            CornerRadius {
                nw: 0,
                ne: 0,
                ..radius
            },
            Color32::from_black_alpha(170),
        );
        let mut job = LayoutJob::single_section(
            file_name(&self.images[idx]),
            TextFormat::simple(FontId::proportional(12.0), Color32::WHITE),
        );
        job.wrap = TextWrapping::truncate_at_width(rect.width() - 16.0);
        let galley = painter.layout_job(job);
        let text_pos = pos2(rect.left() + 8.0, bar.center().y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, Color32::WHITE);
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, ui.visuals().selection.stroke.color),
            StrokeKind::Inside,
        );
        response
    }

    /// Paints the thumbnail of image `idx` fitted and centered into `rect`.
    fn paint_thumb(&mut self, ui: &Ui, idx: usize, rect: Rect) {
        let placeholder = ui.visuals().faint_bg_color;
        let warn = ui.visuals().warn_fg_color;
        let painter = ui.painter();
        match self.thumb(idx) {
            Some(Tex::Ready(tex)) => {
                egui::Image::from_texture(tex)
                    .corner_radius(THUMB_RADIUS)
                    .paint_at(ui, fit(tex.size_vec2(), rect, f32::INFINITY));
            }
            Some(Tex::Failed(_)) => {
                painter.rect_filled(rect, CornerRadius::same(THUMB_RADIUS), placeholder);
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "⚠",
                    FontId::proportional((rect.height() * 0.25).clamp(14.0, 32.0)),
                    warn,
                );
            }
            _ => {
                painter.rect_filled(rect, CornerRadius::same(THUMB_RADIUS), placeholder);
            }
        }
    }

    fn image_view(&mut self, ui: &mut Ui, idx: usize) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click());
        if response.double_clicked() {
            self.select(None);
            return;
        }
        self.scroll_to_step(ui, rect);
        let Some(idx) = self.selected.filter(|&i| i == idx) else {
            return; // selection changed this frame, draw it next frame
        };

        let area = rect.shrink(12.0);
        let path = &self.images[idx];
        match self.fulls.get(path) {
            Some(Tex::Ready(tex)) => {
                // At most 1:1 with physical pixels, so small images stay sharp on HiDPI.
                let max_scale = 1.0 / ui.ctx().pixels_per_point();
                egui::Image::from_texture(tex).paint_at(ui, fit(tex.size_vec2(), area, max_scale));
            }
            Some(Tex::Failed(e)) => {
                let msg = format!("Could not load {}\n{e}", file_name(path));
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    msg,
                    FontId::proportional(14.0),
                    ui.visuals().error_fg_color,
                );
            }
            _ => {
                // Show the (blurry) thumbnail as placeholder while the full image loads.
                if let Some(Tex::Ready(tex)) = self.thumbs.get(path) {
                    egui::Image::from_texture(tex)
                        .paint_at(ui, fit(tex.size_vec2(), area, f32::INFINITY));
                }
                egui::Spinner::new()
                    .size(32.0)
                    .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(32.0)));
            }
        }
    }

    fn filmstrip(&mut self, ui: &mut Ui, selected: usize) {
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        self.scroll_to_step(ui, rect);

        let thumb = rect.height();
        let step = thumb + 10.0;
        // Number of neighbours that fit on each side of the centered selection.
        let side = ((rect.width() / 2.0 - thumb / 2.0) / step).floor().max(0.0) as isize;

        let mut clicked = None;
        for offset in -side..=side {
            let Some(idx) = selected
                .checked_add_signed(offset)
                .filter(|&i| i < self.images.len())
            else {
                continue;
            };
            let center = pos2(rect.center().x + offset as f32 * step, rect.center().y);
            let cell = Rect::from_center_size(center, Vec2::splat(thumb));
            let response = ui
                .interact(cell, ui.scope_id().with(("filmstrip", idx)), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text_at_pointer(file_name(&self.images[idx]));

            if response.hovered() && offset != 0 {
                ui.painter().rect_filled(
                    cell,
                    CornerRadius::same(THUMB_RADIUS + 2),
                    ui.visuals().widgets.hovered.weak_bg_fill,
                );
            }
            self.paint_thumb(ui, idx, cell.shrink(4.0));
            if offset == 0 {
                ui.painter().rect_stroke(
                    cell.shrink(1.0),
                    CornerRadius::same(THUMB_RADIUS + 2),
                    Stroke::new(2.0, ui.visuals().selection.stroke.color),
                    StrokeKind::Inside,
                );
            } else if !response.hovered() {
                // Dim the neighbours so the current image stands out.
                ui.painter().rect_filled(
                    cell.shrink(4.0),
                    CornerRadius::same(THUMB_RADIUS),
                    ui.visuals().panel_fill.gamma_multiply(0.45),
                );
            }

            if response.clicked() {
                clicked = Some(idx);
            }
        }

        if clicked.is_some() {
            self.select(clicked);
        }
    }

    /// The folder or image the installed copy should open after Install/Update.
    #[cfg(any(target_os = "linux", windows))]
    fn open_path(&self) -> PathBuf {
        match self.selected {
            Some(i) => self.images[i].clone(),
            None => self.current_dir.clone(),
        }
    }

    /// Installs, then continues in the installed copy at the same folder/image.
    #[cfg(any(target_os = "linux", windows))]
    fn install(&mut self, ctx: &egui::Context) {
        let open = self.open_path();
        match crate::install::install().and_then(|t| crate::install::launch(&t, &open)) {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => self.error = Some(("Install failed", e)),
        }
    }

    /// Downloads and installs `version` in the background.
    #[cfg(any(target_os = "linux", windows))]
    fn start_update(&mut self, ctx: &egui::Context, version: semver::Version) {
        self.updating = true;
        self.update_rx = Some(crate::update::install(ctx, version, self.open_path()));
    }

    /// Handles the result of an update check or installation.
    #[cfg(any(target_os = "linux", windows))]
    fn poll_update(&mut self, ctx: &egui::Context) {
        use crate::update::Msg;
        let Some(msg) = self.update_rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.update_rx = None;
        match msg {
            Msg::Checked(latest) => {
                self.update.last_check = crate::update::now();
                self.update.latest = latest;
                self.update.save();
            }
            // The installed copy opens its own window.
            Msg::Installed => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Msg::Failed(e) => {
                self.updating = false;
                self.error = Some(("Update failed", e));
            }
        }
    }

    /// The Settings menu: icon size, update check and desktop integration.
    fn settings_menu(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Icon size").weak());
        for size in IconSize::ALL {
            let label = format!("{} ({:.0}%)", size.name(), size.scale() * 100.0);
            ui.radio_value(&mut self.icon_size, size, label);
        }
        #[cfg(any(target_os = "linux", windows))]
        self.platform_settings(ui);
    }

    /// Settings menu sections for the platforms with installation and updates.
    #[cfg(any(target_os = "linux", windows))]
    fn platform_settings(&mut self, ui: &mut Ui) {
        ui.separator();
        ui.label(RichText::new("Updates").weak());
        if self.update_eligible {
            self.update_settings(ui);
        } else {
            ui.label("Only the installed copy checks for updates.");
        }

        ui.separator();
        ui.label(RichText::new("Desktop integration").weak());
        let installed = crate::install::installed_exe().ok();
        if self.offer_install {
            let (label, tooltip) = if crate::install::has_installed_copy() {
                (
                    "Update installation",
                    "Replace the installed copy with this version",
                )
            } else {
                (
                    "Install for this user",
                    "Install in your user folder and application menu, \
                     so you can delete this download",
                )
            };
            let icon = egui::Image::new(INSTALL_ICON)
                .tint(ui.visuals().text_color())
                .fit_to_exact_size(Vec2::splat(16.0));
            if ui
                .add(egui::Button::image_and_text(icon, label))
                .on_hover_text(tooltip)
                .clicked()
            {
                self.install(ui.ctx());
                ui.close();
            }
        } else if let Some(path) = installed {
            ui.label(format!("Installed in {}", path.display()));
        }
    }

    /// "Check for updates" switch and its status (installed copy only).
    #[cfg(any(target_os = "linux", windows))]
    fn update_settings(&mut self, ui: &mut Ui) {
        let changed = ui
            .checkbox(&mut self.update.check, "Check for updates once a day")
            .on_hover_text(
                "Looks for a new version on GitHub when LeoView starts. \
                 Nothing is installed without your click.",
            )
            .changed();
        if changed {
            self.update.save();
            // Turning it on checks right away; off cancels a running check.
            self.update_rx =
                (self.update.check && !self.updating).then(|| crate::update::check(ui.ctx()));
        }
        if !self.update.check || self.updating {
            return;
        }
        if self.update_rx.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Checking…").weak());
            });
        } else if let Some(version) = self.update.offered() {
            ui.label(format!("Version {version} is available."));
        } else if self.update.last_check > 0 {
            ui.label(RichText::new("LeoView is up to date.").weak());
        }
    }

    fn error_window(&mut self, ctx: &egui::Context) {
        let Some((title, error)) = &self.error else {
            return;
        };
        let mut open = true;
        egui::Window::new(*title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(error);
            });
        if !open {
            self.error = None;
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let logo = self.logo.clone();
        let mut show = self.show_about;
        egui::Window::new("About")
            .open(&mut show)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Image::from_texture(&logo)
                            .fit_to_exact_size(Vec2::splat(96.0))
                            .corner_radius(22),
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new("LeoView").size(20.0).strong());
                    ui.label(RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION"))).weak());
                    ui.add_space(6.0);
                    ui.label("A simple image viewer written in Rust with egui.");
                });
            });
        self.show_about &= show;
    }
}

impl eframe::App for LeoView {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(ICON_SIZE_KEY, self.icon_size.name().to_owned());
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        set_icon_scale(&ctx, self.icon_size.scale());
        self.poll_loader(&ctx);
        self.handle_keys(&ctx);
        #[cfg(any(target_os = "linux", windows))]
        self.poll_update(&ctx);

        let style = ui.style().clone();
        let panel = Frame::side_top_panel(&style);
        let viewer_bg = ui.visuals().extreme_bg_color;

        egui::Panel::top("toolbar")
            .frame(panel.inner_margin(Margin::symmetric(12, 8)))
            .exact_size(TOOLBAR_HEIGHT * self.icon_size.scale())
            .show(ui, |ui| self.toolbar(ui));
        if self.show_sidebar {
            egui::Panel::left("file_browser")
                .frame(panel.inner_margin(Margin::same(12)))
                .resizable(true)
                .default_size(230.0)
                .size_range(160.0..=480.0)
                .show(ui, |ui| self.file_browser(ui));
        }
        if let Some(selected) = self.selected {
            egui::Panel::bottom("filmstrip")
                .frame(panel.inner_margin(Margin::symmetric(12, 10)))
                .exact_size(FILMSTRIP_HEIGHT)
                .show(ui, |ui| self.filmstrip(ui, selected));
        }
        let central = match self.selected {
            Some(_) => Frame::central_panel(&style).inner_margin(0).fill(viewer_bg),
            None => Frame::central_panel(&style).inner_margin(Margin::same(16)),
        };
        let central =
            egui::CentralPanel::default()
                .frame(central)
                .show(ui, |ui| match self.selected {
                    Some(idx) => self.image_view(ui, idx),
                    None => self.grid_view(ui),
                });
        self.viewer_center_x = Some(central.response.rect.center().x);

        self.about_window(&ctx);
        self.error_window(&ctx);
        self.take_screenshot_when_ready(&ctx);
    }
}

// ---- helpers ----

fn setup_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        let spacing = &mut style.spacing;
        spacing.item_spacing = vec2(8.0, 6.0);
        spacing.button_padding = vec2(10.0, 4.0);
        spacing.interact_size.y = 26.0;
        spacing.window_margin = Margin::same(16);

        let visuals = &mut style.visuals;
        let radius = CornerRadius::same(6);
        visuals.widgets.noninteractive.corner_radius = radius;
        visuals.widgets.inactive.corner_radius = radius;
        visuals.widgets.hovered.corner_radius = radius;
        visuals.widgets.active.corner_radius = radius;
        visuals.widgets.open.corner_radius = radius;
        visuals.window_corner_radius = CornerRadius::same(10);
        // Warm accent to match the lioness icon.
        visuals.selection.stroke.color = Color32::from_rgb(0xE0, 0xA8, 0x45);
        visuals.selection.bg_fill = Color32::from_rgb(0xB0, 0x7A, 0x2C);
    });
}

/// A texture of an embedded PNG, smoothly scaled down for small sizes.
fn load_logo(ctx: &egui::Context, name: &str, png: &[u8]) -> TextureHandle {
    let logo = image::load_from_memory(png)
        .expect("embedded icon is a valid PNG")
        .to_rgba8();
    let logo = ColorImage::from_rgba_unmultiplied(
        [logo.width() as usize, logo.height() as usize],
        logo.as_raw(),
    );
    ctx.load_texture(
        name,
        logo,
        TextureOptions {
            mipmap_mode: Some(egui::TextureFilter::Linear),
            ..TextureOptions::LINEAR
        },
    )
}

fn icon_scale_id() -> egui::Id {
    egui::Id::unique("leoview_icon_scale")
}

/// Makes the icon size setting's scale available to the icon helpers below. Set at
/// the start of every frame.
fn set_icon_scale(ctx: &egui::Context, scale: f32) {
    ctx.data_mut(|d| d.insert_temp(icon_scale_id(), scale));
}

/// The factor icons and their buttons are scaled by, from the icon size setting.
fn icon_scale(ui: &Ui) -> f32 {
    ui.data(|d| d.get_temp(icon_scale_id())).unwrap_or(1.0)
}

fn icon_button_widget(ui: &Ui, icon: &str) -> egui::Button<'static> {
    let scale = icon_scale(ui);
    egui::Button::new(RichText::new(icon.to_owned()).size(14.0 * scale))
        .min_size(ICON_BUTTON * scale)
}

/// A fixed-size button showing an SVG `icon`, tinted to the button's text color.
fn svg_button(ui: &mut Ui, icon: egui::ImageSource, tooltip: &str) -> egui::Response {
    let scale = icon_scale(ui);
    let response = ui.add(egui::Button::new("").min_size(ICON_BUTTON * scale));
    let color = ui.style().interact(&response).fg_stroke.color;
    paint_icon(ui, response.rect, icon, 18.0 * scale, color);
    response.on_hover_text(tooltip)
}

/// Draws `icon` at `size` points, centered in `rect`, in `color`.
fn paint_icon(ui: &Ui, rect: Rect, icon: egui::ImageSource, size: f32, color: Color32) {
    egui::Image::new(icon)
        .tint(color)
        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(size)));
}

/// Buttons for following the system theme or picking dark or light, like
/// [`egui::global_theme_preference_buttons`] but with the icons shared with xpaint.
fn theme_buttons(ui: &mut Ui) {
    use egui::ThemePreference;
    let mut preference = ui.options(|o| o.theme_preference);
    // A scope keeps the toolbar's centered row, so the buttons line up with the
    // others at every icon size (a nested `horizontal` row is only as tall as text).
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // Right to left, so the order on screen is System, Dark, Light.
        for (choice, icon, tip) in [
            (
                ThemePreference::Light,
                THEME_LIGHT_ICON,
                "Use the light theme",
            ),
            (ThemePreference::Dark, THEME_DARK_ICON, "Use the dark theme"),
            (
                ThemePreference::System,
                THEME_SYSTEM_ICON,
                "Follow the system theme",
            ),
        ] {
            let selected = preference == choice;
            let scale = icon_scale(ui);
            let button = egui::Button::selectable(selected, "").min_size(ICON_BUTTON * scale);
            let response = ui.add(button);
            // White on the selected (accent) background, like xpaint.
            let color = if selected {
                Color32::WHITE
            } else {
                ui.style()
                    .interact_selectable(&response, false)
                    .fg_stroke
                    .color
            };
            paint_icon(ui, response.rect, icon, 16.0 * scale, color);
            if response.on_hover_text(tip).clicked() {
                preference = choice;
            }
        }
    });
    ui.ctx().set_theme(preference);
}

/// A fixed-size button with a centered icon glyph.
fn icon_button(ui: &mut Ui, icon: &str, enabled: bool, tooltip: &str) -> egui::Response {
    let button = icon_button_widget(ui, icon);
    ui.add_enabled(enabled, button).on_hover_text(tooltip)
}

/// Like [`Ui::put`], but the widget can be disabled.
fn put_enabled(
    ui: &mut Ui,
    rect: Rect,
    enabled: bool,
    widget: impl egui::Widget,
) -> egui::Response {
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::centered_and_justified(egui::Direction::TopDown)),
        |ui| ui.add_enabled(enabled, widget),
    )
    .inner
}

fn empty_state(ui: &mut Ui, icon: &str, text: &str, color: Color32) {
    ui.with_layout(
        Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.label(
                RichText::new(format!("{icon}\n\n{text}"))
                    .size(15.0)
                    .color(color),
            );
        },
    );
}

fn count_label(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn default_dir() -> PathBuf {
    // The platform's pictures folder: XDG on Linux, the (possibly redirected) known folder on Windows.
    let home = std::env::home_dir();
    dirs::picture_dir()
        .or_else(|| home.as_ref().map(|h| h.join("Pictures")))
        .filter(|p| p.is_dir())
        .or(home)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from(std::path::MAIN_SEPARATOR_STR))
}

/// Dotfiles everywhere, plus entries with the hidden or system attribute on Windows
/// (e.g. `$RECYCLE.BIN`, `System Volume Information`).
fn is_hidden(entry: &std::fs::DirEntry) -> bool {
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
        };
        // On Windows this comes from the directory listing itself, so it is cheap.
        if let Ok(meta) = entry.metadata() {
            return meta.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0;
        }
    }
    false
}

/// Drive roots like `C:\`, so the browser can leave the drive it started on.
#[cfg(windows)]
fn list_drives() -> Vec<PathBuf> {
    // SAFETY: takes no arguments and only returns a bitmask.
    let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| PathBuf::from(format!("{}:\\", (b'A' + i) as char)))
        .collect()
}

#[cfg(not(windows))]
fn list_drives() -> Vec<PathBuf> {
    Vec::new()
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn sort_key(path: &Path) -> String {
    file_name(path).to_lowercase()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Scales `size` to fit inside `bounds` keeping the aspect ratio, centered.
/// The scale factor never exceeds `max_scale`.
fn fit(size: Vec2, bounds: Rect, max_scale: f32) -> Rect {
    let scale = (bounds.width() / size.x)
        .min(bounds.height() / size.y)
        .min(max_scale);
    Rect::from_center_size(bounds.center(), size * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_size_scales_buttons_and_is_stored_by_name() {
        let ctx = egui::Context::default();
        for size in IconSize::ALL {
            assert_eq!(IconSize::from_name(size.name()), Some(size));
            set_icon_scale(&ctx, size.scale());
            let mut width = 0.0;
            let mut output = ctx.run_ui(Default::default(), |ui| {
                width = svg_button(ui, ABOUT_ICON, "About").rect.width();
            });
            // No renderer to apply the texture changes to.
            output.textures_delta.clear();
            assert_eq!(width, ICON_BUTTON.x * size.scale());
        }
    }
}
