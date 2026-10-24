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

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "ico", "tga", "qoi", "pnm", "pbm",
    "pgm", "ppm",
];
const TOOLBAR_HEIGHT: f32 = 42.0;
const FILMSTRIP_HEIGHT: f32 = 92.0;
const GRID_SPACING: f32 = 8.0;
/// Assumed aspect ratio while an image's real one is still being read.
const DEFAULT_ASPECT: f32 = 1.5;
const THUMB_RADIUS: u8 = 6;
const ICON_BUTTON: Vec2 = vec2(30.0, 26.0);
/// Accumulated scroll distance that steps to the next/previous image.
const SCROLL_STEP: f32 = 60.0;
/// How many images before/after the selected one are preloaded at full size.
const PRELOAD_RADIUS: usize = 1;

enum Tex {
    Loading,
    Ready(TextureHandle),
    Failed(String),
}

pub struct LeoView {
    loader: Loader,
    logo: TextureHandle,
    current_dir: PathBuf,
    subdirs: Vec<PathBuf>,
    images: Vec<PathBuf>,
    /// `Some` = single image view, `None` = directory (thumbnail grid) view.
    selected: Option<usize>,
    thumb_size: f32,
    thumbs: HashMap<PathBuf, Tex>,
    fulls: HashMap<PathBuf, Tex>,
    dir_error: Option<String>,
    show_about: bool,
    scroll_accum: f32,
    /// Horizontal center of the image area last frame, to align the toolbar arrows.
    viewer_center_x: Option<f32>,
    /// Aspect ratio (width / height) per image, 0.0 while unknown.
    aspects: Vec<f32>,
    /// Dev aid: `LEOVIEW_SCREENSHOT=out.png` saves the app's own frame and quits.
    screenshot_to: Option<PathBuf>,
}

impl LeoView {
    pub fn new(cc: &eframe::CreationContext<'_>, start: Option<PathBuf>) -> Self {
        setup_style(&cc.egui_ctx);

        let logo = image::load_from_memory(ICON_PNG)
            .expect("embedded icon is a valid PNG")
            .to_rgba8();
        let logo = ColorImage::from_rgba_unmultiplied(
            [logo.width() as usize, logo.height() as usize],
            logo.as_raw(),
        );
        let logo = cc.egui_ctx.load_texture(
            "logo",
            logo,
            TextureOptions {
                mipmap_mode: Some(egui::TextureFilter::Linear),
                ..TextureOptions::LINEAR
            },
        );

        let mut app = Self {
            loader: Loader::new(&cc.egui_ctx),
            logo,
            current_dir: PathBuf::new(),
            subdirs: Vec::new(),
            images: Vec::new(),
            selected: None,
            thumb_size: 160.0,
            thumbs: HashMap::new(),
            fulls: HashMap::new(),
            dir_error: None,
            show_about: false,
            scroll_accum: 0.0,
            viewer_center_x: None,
            aspects: Vec::new(),
            screenshot_to: std::env::var_os("LEOVIEW_SCREENSHOT").map(PathBuf::from),
        };

        let start = start
            .and_then(|p| p.canonicalize().ok())
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
                    let path = entry.path();
                    if is_hidden(&path) {
                        continue;
                    }
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
        let delta = ui.input(|i| i.smooth_scroll_delta);
        self.scroll_accum += delta.y + delta.x;
        if self.scroll_accum.abs() >= SCROLL_STEP {
            let dir = if self.scroll_accum > 0.0 { -1 } else { 1 };
            self.scroll_accum = 0.0;
            self.step(dir);
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
            let group_width = 2.0 * ICON_BUTTON.x + counter_width + 2.0 * gap;
            let center_x = self.viewer_center_x.unwrap_or(rect.center().x);
            let group = Rect::from_center_size(
                pos2(center_x, rect.center().y),
                vec2(group_width, ICON_BUTTON.y),
            );

            let prev = Rect::from_min_size(group.min, ICON_BUTTON);
            let next = Rect::from_min_size(
                pos2(group.right() - ICON_BUTTON.x, group.top()),
                ICON_BUTTON,
            );
            let counter_rect = Rect::from_min_max(
                pos2(prev.right() + gap, group.top()),
                pos2(next.left() - gap, group.bottom()),
            );
            if put_enabled(ui, prev, i > 0, icon_button_widget("◀"))
                .on_hover_text("Previous (←)")
                .clicked()
            {
                self.step(-1);
            }
            ui.put(
                counter_rect,
                egui::Label::new(RichText::new(counter).weak()),
            );
            if put_enabled(ui, next, i + 1 < self.images.len(), icon_button_widget("▶"))
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
                        .fit_to_exact_size(Vec2::splat(24.0))
                        .corner_radius(5),
                );
                ui.label(RichText::new("LeoView").strong().size(15.0));
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
                if ui.button("ℹ  About").clicked() {
                    self.show_about = true;
                }
                ui.separator();
                egui::widgets::global_theme_preference_buttons(ui);
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
                }
            });
        });
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

    fn about_window(&mut self, ctx: &egui::Context) {
        let logo = self.logo.clone();
        egui::Window::new("About")
            .open(&mut self.show_about)
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
                    ui.add_space(4.0);
                });
            });
    }
}

impl eframe::App for LeoView {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_loader(&ctx);
        self.handle_keys(&ctx);

        let style = ui.style().clone();
        let panel = Frame::side_top_panel(&style);
        let viewer_bg = ui.visuals().extreme_bg_color;

        egui::Panel::top("toolbar")
            .frame(panel.inner_margin(Margin::symmetric(12, 8)))
            .exact_size(TOOLBAR_HEIGHT)
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::left("file_browser")
            .frame(panel.inner_margin(Margin::same(12)))
            .resizable(true)
            .default_size(230.0)
            .size_range(160.0..=480.0)
            .show(ui, |ui| self.file_browser(ui));
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

fn icon_button_widget(icon: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(icon).size(14.0)).min_size(ICON_BUTTON)
}

/// A fixed-size button with a centered icon glyph.
fn icon_button(ui: &mut Ui, icon: &str, enabled: bool, tooltip: &str) -> egui::Response {
    ui.add_enabled(enabled, icon_button_widget(icon))
        .on_hover_text(tooltip)
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
    let home = std::env::home_dir();
    home.as_ref()
        .map(|h| h.join("Pictures"))
        .filter(|p| p.is_dir())
        .or(home)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
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
