//! Picture grid: thumbnails, click / Shift / Ctrl selection, rubber-band
//! selection from empty space, dragging pictures out to other programs, and the
//! right-click menu shared by all views.

use std::collections::HashSet;

use eframe::egui::{self, Color32, Modifiers, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::panels::paint_fit;
use crate::i18n::tr;
use super::*;

/// Rubber-band selection. Positions are in grid content coordinates
/// (y includes the scroll offset), so it keeps working while scrolling.
pub(super) struct Marquee {
    start: Pos2,
    cur: Pos2,
    base: HashSet<String>,
    /// albums selected before the band started (Ctrl / Shift)
    abase: HashSet<u32>,
    moved: bool,
}

/// Empty border around each cell: the gaps between cells answer no click
/// (a press there starts the rubber band, a click clears the selection).
const GAP: f32 = 6.0;

impl App {
    pub(super) fn ui_grid(&mut self, ui: &mut egui::Ui) {
        // thumb_size = picture height; cell width follows the usual shape of the shown pictures
        let size = self.settings.thumb_size.round();
        let img = vec2((size * self.aspect).round(), size);
        let label_h = if self.settings.show_names { 18.0 + UI_FONT_PLUS } else { 0.0 };
        let avail_w = ui.available_width() - 14.0;
        let cols = ((avail_w / (img.x + 10.0 + 2.0 * GAP)).floor() as usize).max(1);
        let cell = vec2((avail_w / cols as f32).floor(), size + label_h + 10.0 + 2.0 * GAP);
        if (cols != self.cols || (cell.y - self.row_h).abs() > 0.5) && self.focus.is_some() && self.scroll_to.is_none() {
            // window / panel / thumbnail size changed: keep the focused picture in view
            self.scroll_to = self.focus;
        }
        self.cols = cols;
        self.row_h = cell.y;
        // album view: the albums come first, as folder cells, then the pictures
        let lead = self.album_lead();
        let nl = lead.len();
        let n = nl + self.shown.len();
        let rows = n.div_ceil(cols);
        // the grid's empty space (gaps included): right-click menu in the album view.
        // Added before the cells, so a cell under the pointer takes the click first.
        let bg = ui.interact(ui.available_rect_before_wrap(), egui::Id::new("grid_bg"), Sense::click());
        self.mark("grid_bg", bg.rect);
        if self.view == View::Albums {
            bg.context_menu(|ui| self.empty_menu(ui));
        }
        if n == 0 {
            ui.centered_and_justified(|ui| {
                let msg = if self.view == View::Albums {
                    tr("비어 있습니다.\n그림을 여기(위 앨범 이름·왼쪽 앨범)로 끌어다 놓거나, Ctrl+C·Ctrl+X 한 뒤 여기서 Ctrl+V\n빈 곳 오른쪽 클릭·Ctrl+N: 새 앨범", "Empty.\nDrag pictures here (or onto an album name above / on the left), or Ctrl+C / Ctrl+X and then Ctrl+V here\nRight-click empty space or Ctrl+N: new album")
                } else if self.db.images.is_empty() {
                    tr("imgs 폴더가 비어 있습니다.\n＋ 가져오기, 창에 그림 끌어다 놓기, 또는 탐색기에서 복사한 뒤 Ctrl+V", "The imgs folder is empty.\n＋ Import, drag pictures into the window, or copy in Explorer and press Ctrl+V")
                } else {
                    tr("검색 조건에 맞는 그림이 없습니다.", "No pictures match the search.")
                };
                ui.label(egui::RichText::new(msg).color(DIM));
            });
            return;
        }
        let mut sa = egui::ScrollArea::vertical().auto_shrink(false).id_salt("grid");
        if let Some(y) = self.force_scroll.take() {
            sa = sa.vertical_scroll_offset(y.max(0.0));
        } else if let Some(g) = self.scroll_to.map(|i| i + nl).or(self.scroll_to_cell).filter(|_| self.view_h > 0.0) {
            // (view height is unknown on the very first frame: wait for it)
            self.scroll_to = None;
            self.scroll_to_cell = None;
            let top = (g / cols) as f32 * cell.y;
            let bottom = top + cell.y;
            if top < self.scroll_offset {
                sa = sa.vertical_scroll_offset(top);
            } else if bottom > self.scroll_offset + self.view_h {
                sa = sa.vertical_scroll_offset(bottom - self.view_h);
            }
        }
        let mut clicks: Vec<(usize, Modifiers, bool)> = Vec::new();
        let late_dbl = ui.input(|i| i.time) - self.album_click_open_t < 0.7;
        let mut tile_clicks: Vec<(usize, Modifiers, bool)> = Vec::new();
        let mut img_rects: Vec<(usize, Rect)> = Vec::new();
        let mut tile_rects: Vec<(usize, Rect)> = Vec::new();
        let mut hit: Vec<Rect> = Vec::new();
        let mut menu_open_for: Option<usize> = None;
        let out = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                sa.show_rows(ui, cell.y, rows, |ui, range| {
                    for r in range {
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            for c in 0..cols {
                                let pos = r * cols + c;
                                if pos >= n {
                                    break;
                                }
                                if pos < nl {
                                    let (slot, _) = ui.allocate_exact_size(cell, Sense::hover());
                                    let rect = slot.shrink(GAP);
                                    let resp = ui.interact(rect, egui::Id::new(("album_cell", lead[pos])), Sense::click());
                                    self.draw_album_cell(ui, rect, &resp, lead[pos], img, self.album_focus == Some(pos));
                                    tile_rects.push((pos, rect));
                                    hit.push(rect);
                                    self.album_drop.push((lead[pos], rect.intersect(ui.clip_rect())));
                                    if resp.double_clicked() && !late_dbl {
                                        tile_clicks.push((pos, Modifiers::NONE, true));
                                    } else if resp.clicked() && !(late_dbl && resp.double_clicked()) {
                                        // a plain click opens the album (like a folder in Explorer's tree);
                                        // Ctrl / Shift click selects
                                        let m = ui.input(|i| i.modifiers);
                                        tile_clicks.push((pos, m, !m.ctrl && !m.shift));
                                    }
                                    if let Some(id) = lead[pos] {
                                        if resp.secondary_clicked() && !self.album_sel.contains(&id) {
                                            tile_clicks.push((pos, Modifiers::NONE, false));
                                        }
                                        resp.context_menu(|ui| self.album_menu(ui));
                                    }
                                    continue;
                                }
                                let pos = pos - nl;
                                // a menu action (search, trash…) can shorten the list mid-loop
                                if pos >= self.shown.len() {
                                    break;
                                }
                                let (slot, _) = ui.allocate_exact_size(cell, Sense::hover());
                                let rect = slot.shrink(GAP);
                                let resp = ui.interact(rect, egui::Id::new(("pic_cell", pos)), Sense::click());
                                hit.push(rect);
                                let ir = self.draw_cell(ui, rect, &resp, pos, img);
                                img_rects.push((pos, ir));
                                if late_dbl && resp.double_clicked() {
                                    // 2nd click of a double click that already opened an album: nothing
                                } else if resp.double_clicked() {
                                    clicks.push((pos, Modifiers::NONE, true));
                                } else if resp.clicked() {
                                    clicks.push((pos, ui.input(|i| i.modifiers), false));
                                }
                                if resp.secondary_clicked() {
                                    let id = self.db.images[self.shown[pos]].id.clone();
                                    if !self.sel.contains(&id) {
                                        clicks.push((pos, Modifiers::NONE, false));
                                    }
                                }
                                if resp.context_menu_opened() || resp.secondary_clicked() {
                                    menu_open_for = Some(pos);
                                }
                                resp.context_menu(|ui| self.image_menu(ui));
                            }
                        });
                    }
                })
            })
            .inner;
        self.scroll_offset = out.state.clamped_offset().y;
        let inner = out.inner_rect;
        self.view_h = inner.height();
        self.mark("grid", inner);
        let _ = menu_open_for;
        for (g, m, dbl) in tile_clicks {
            if dbl {
                // lead[g]: Some(id) = that album, None = 무소속
                self.album_click_open_t = ui.input(|i| i.time);
                self.open_album(Some(lead[g]));
            } else {
                self.click_album(g, &lead, m);
            }
        }
        for (pos, m, dbl) in clicks {
            if pos >= self.shown.len() {
                continue;
            }
            self.click(pos, m);
            if dbl {
                self.open_viewer(if self.view == View::Albums { View::Albums } else { View::Grid });
            }
        }
        self.grid_mouse(ui, inner, cell, cols, &img_rects, &tile_rects, &lead, &hit);
    }

    /// Rubber band + drag-out handling for the grid.
    #[allow(clippy::too_many_arguments)]
    fn grid_mouse(&mut self, ui: &egui::Ui, inner: Rect, cell: Vec2, cols: usize, img_rects: &[(usize, Rect)], tile_rects: &[(usize, Rect)], lead: &[Option<u32>], hit: &[Rect]) {
        let nl = lead.len();
        if self.modal.is_some() || ui.ctx().any_popup_open() {
            self.marquee = None;
            self.press = None;
            return;
        }
        let (pos, pressed, down, released, mods) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_down(egui::PointerButton::Primary),
                i.pointer.button_released(egui::PointerButton::Primary),
                i.modifiers,
            )
        });
        let Some(pos) = pos else { return };
        let so = self.scroll_offset;
        let to_content = move |p: Pos2| pos2(p.x, p.y - inner.top() + so);
        if pressed && inner.contains(pos) && pos.x < inner.max.x - 14.0 {
            // a press on a picture that is already selected may drag the pictures out (Explorer, upload box);
            // anywhere else (an unselected picture or the gaps) starts the rubber band
            // (press keeps a grid index: albums first, then pictures)
            let on_sel = img_rects.iter().find(|(_, r)| r.contains(pos)).filter(|(p, _)| *p < self.shown.len() && self.sel.contains(&self.db.images[self.shown[*p]].id)).map(|(p, _)| p + nl);
            // albums: a plain press on any album may drag it (a click on it opens it, so it is rarely selected)
            let plain = !mods.ctrl && !mods.shift;
            let on_sel_album = tile_rects.iter().find(|(_, r)| r.contains(pos)).filter(|(g, _)| lead[*g].is_some_and(|a| plain || self.album_sel.contains(&a))).map(|(g, _)| *g);
            if let Some(g) = on_sel.or(on_sel_album) {
                self.press = Some((pos, g));
            } else {
                let keep = mods.ctrl || mods.shift;
                let base = if keep { self.sel.clone() } else { HashSet::new() };
                let abase = if keep { self.album_sel.clone() } else { HashSet::new() };
                self.marquee = Some(Marquee { start: to_content(pos), cur: to_content(pos), base, abase, moved: false });
            }
        }
        // drag pictures: inside the window onto an album, out of it to Explorer / a browser upload box
        if let Some((p0, cellpos)) = self.press {
            if down && pos.distance(p0) > 8.0 {
                self.press = None;
                if let Some(a) = lead.get(cellpos).copied().flatten().filter(|a| !self.album_sel.contains(a)) {
                    self.sel.clear();
                    self.album_sel.clear();
                    self.album_sel.insert(a);
                }
                if cellpos >= nl && cellpos - nl < self.shown.len() {
                    let p = cellpos - nl;
                    let id = self.db.images[self.shown[p]].id.clone();
                    if !self.sel.contains(&id) {
                        self.click(p, Modifiers::NONE);
                    }
                }
                // pictures and albums that are selected move together
                let ids = self.picked_pictures();
                let albums: Vec<u32> = self.album_sel.iter().copied().collect();
                if !ids.is_empty() || !albums.is_empty() {
                    self.pic_drag = Some(ids);
                    self.album_drag = albums;
                }
            }
            if released {
                self.press = None;
            }
        }
        let Some(mq) = self.marquee.as_mut() else { return };
        if down {
            mq.cur = to_content(pos);
            if mq.cur.distance(mq.start) > 4.0 {
                mq.moved = true;
            }
            // scroll while dragging near the top / bottom edge
            if pos.y > inner.bottom() - 24.0 {
                self.force_scroll = Some(self.scroll_offset + 18.0);
            } else if pos.y < inner.top() + 24.0 && self.scroll_offset > 0.0 {
                self.force_scroll = Some(self.scroll_offset - 18.0);
            }
        }
        let mq = self.marquee.as_ref().unwrap();
        if mq.moved {
            let r = Rect::from_two_pos(mq.start, mq.cur);
            let left0 = inner.left() + 4.0;
            let n = nl + self.shown.len();
            let r0 = (r.top() / cell.y).floor().max(0.0) as usize;
            let r1 = (r.bottom() / cell.y).floor().max(0.0) as usize;
            let mut sel = mq.base.clone();
            let mut asel = mq.abase.clone();
            for row in r0..=r1 {
                for c in 0..cols {
                    let p = row * cols + c;
                    if p >= n {
                        break;
                    }
                    let cr = Rect::from_min_size(pos2(left0 + c as f32 * cell.x, row as f32 * cell.y), cell).shrink(GAP);
                    if cr.intersects(r) {
                        if p < nl {
                            if let Some(a) = lead[p] {
                                asel.insert(a);
                            }
                        } else {
                            sel.insert(self.db.images[self.shown[p - nl]].id.clone());
                        }
                    }
                }
            }
            self.album_sel = asel;
            // paint the band
            let screen = Rect::from_two_pos(pos2(r.left(), r.top() + inner.top() - self.scroll_offset), pos2(r.right(), r.bottom() + inner.top() - self.scroll_offset));
            let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("marquee"))).with_clip_rect(inner);
            p.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(90, 130, 190, 40));
            p.rect_stroke(screen, 0.0, Stroke::new(1.0, SEL_STROKE), egui::StrokeKind::Inside);
            let first = self.shown.iter().position(|&i| sel.contains(&self.db.images[i].id));
            self.sel = sel;
            if first.is_some() {
                self.focus = first;
            }
        }
        if released || !down {
            let mq = self.marquee.take().unwrap();
            if !mq.moved && !mods.ctrl && !mods.shift && !img_rects.iter().any(|(_, r)| r.contains(pos)) {
                // a click on empty space (a gap or past the last cell) clears the selection
                let on_cell = hit.iter().any(|r| r.contains(pos));
                if !on_cell {
                    self.sel.clear();
                    self.album_sel.clear();
                }
            }
        }
    }

    fn draw_cell(&mut self, ui: &egui::Ui, rect: Rect, resp: &egui::Response, pos: usize, img: Vec2) -> Rect {
        let ix = self.shown[pos];
        let (id, score, ntags, missing, name) = {
            let im = &self.db.images[ix];
            (im.id.clone(), im.score, im.tags.len(), im.missing, im.name().to_string())
        };
        let selected = self.sel.contains(&id);
        let focused = self.focus == Some(pos);
        let p = ui.painter().clone();
        let inner = rect.shrink(2.0);
        if selected {
            p.rect_filled(inner, 4.0, SEL);
            p.rect_stroke(inner, 4.0, Stroke::new(1.5, SEL_STROKE), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(inner, 4.0, CELL_HOVER);
        }
        if focused {
            p.rect_stroke(inner.shrink(1.0), 4.0, Stroke::new(1.0, FOCUS_STROKE), egui::StrokeKind::Inside);
        }
        let img_rect = Rect::from_min_size(pos2(rect.center().x - img.x / 2.0, rect.top() + 5.0), img);
        let shown_r = match self.thumb(&id) {
            Some(t) => paint_fit(&p, img_rect, &t),
            None => {
                let r = img_rect.shrink(img.x.min(img.y) * 0.2);
                p.rect_filled(r, 4.0, Color32::from_rgb(36, 36, 40));
                if missing {
                    p.text(img_rect.center(), egui::Align2::CENTER_CENTER, tr("파일 없음", "File missing"), egui::FontId::proportional(13.0 + UI_FONT_PLUS), ERR);
                }
                img_rect
            }
        };
        if score > 0 {
            let c = pos2(shown_r.left() + 13.0, shown_r.top() + 13.0);
            p.circle_filled(c, 11.0, score_color(score));
            p.circle_stroke(c, 11.0, Stroke::new(1.0, Color32::from_black_alpha(160)));
            p.text(c, egui::Align2::CENTER_CENTER, score.to_string(), egui::FontId::proportional(14.0), Color32::WHITE);
        }
        if ntags > 0 {
            let r = Rect::from_min_size(pos2(shown_r.right() - 26.0, shown_r.top() + 4.0), vec2(22.0, 16.0));
            p.rect_filled(r, 3.0, TAG_BG.gamma_multiply(0.95));
            p.text(r.center(), egui::Align2::CENTER_CENTER, format!("#{ntags}"), egui::FontId::proportional(11.0), TEXT);
        }
        if self.settings.show_names {
            let mut job = egui::text::LayoutJob::simple(name, egui::FontId::proportional(11.5 + UI_FONT_PLUS), DIM, rect.width() - 8.0);
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            job.halign = egui::Align::Center;
            let g = ui.fonts_mut(|f| f.layout_job(job));
            p.galley(pos2(rect.center().x, img_rect.bottom() + 2.0), g, DIM);
        }
        if resp.hovered() && self.marquee.is_none() {
            let im = &self.db.images[ix];
            let mut tip = format!("{}\n{}×{}  {}", im.name(), im.w, im.h, human_size(im.size));
            if !im.preset.is_empty() || !im.ckpt.is_empty() {
                tip.push_str(&format!("\n{}  {}", im.preset, im.ckpt));
            }
            if let Some(s) = im.seed {
                tip.push_str(&format!("\nseed {s}"));
            }
            if im.score > 0 {
                tip.push_str(&trf!("\n{}점", "\nscore {}", im.score));
            }
            if !im.tags.is_empty() {
                tip.push_str(&format!("\n{}", im.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")));
            }
            resp.clone().on_hover_text(tip);
        }
        img_rect
    }

    /// Right-click menu for the selected pictures (all views use it).
    /// A picture drag in progress (started in the grid): follow the pointer,
    /// light up the album under it, put the pictures there on release. Leaving
    /// the window hands the drag to Windows (Explorer, upload boxes).
    pub(super) fn pic_drag_frame(&mut self, ctx: &egui::Context) {
        let Some(ids) = self.pic_drag.clone() else { return };
        let albums = self.album_drag.clone();
        let (pos, down, released, shift, ctrl) = ctx.input(|i| {
            (i.pointer.latest_pos(), i.pointer.button_down(egui::PointerButton::Primary), i.pointer.button_released(egui::PointerButton::Primary), i.modifiers.shift, i.modifiers.ctrl)
        });
        // album view: a drop moves the pictures out of the album shown (Ctrl: copy, they stay here too);
        // elsewhere a drop adds them (Shift: move)
        let in_albums = self.view == View::Albums;
        let moves = |t: Option<u32>, from: Option<Option<u32>>| -> bool {
            matches!(from, Some(f) if f != t) && t.is_some() && if in_albums { !ctrl } else { shift }
        };
        // an album being dragged is no place to drop it on
        let target = pos.and_then(|p| self.album_drop.iter().rev().find(|(a, r)| r.contains(p) && !a.is_some_and(|x| albums.contains(&x))).map(|x| x.0));
        if released || !down {
            self.pic_drag = None;
            self.album_drag.clear();
            if let Some(t) = target {
                if !albums.is_empty() {
                    match t {
                        Some(a) => self.move_albums(&albums, a),
                        None => self.notice(tr("앨범은 '무소속' 안에 둘 수 없습니다", "Albums cannot be put inside 'Unassigned'"), true),
                    }
                }
                if !ids.is_empty() {
                    // at the album view's top the pictures come from the top (album 0)
                    let from = if in_albums { self.here() } else { self.cur_album() };
                    if moves(t, from) {
                        self.move_to_album(&ids, from, t);
                    } else {
                        self.add_to_album(&ids, t);
                    }
                }
            }
            return;
        }
        let outside = pos.is_none_or(|p| !ctx.content_rect().shrink(3.0).contains(p));
        if outside && !self.offscreen && winshell::mouse_button_down() {
            self.pic_drag = None;
            self.album_drag.clear();
            let paths = self.paths_of(&ids);
            if !paths.is_empty() && winshell::drag_files(&paths) {
                self.notice(trf!("{}장을 끌어다 놓음", "Dropped {} pictures", paths.len()), false);
            }
            // the OS kept the button-up event; tell egui the button is free again
            let p = pos.unwrap_or(Pos2::ZERO);
            self.inject.push_back(InjectStep {
                events: vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
                mods: None,
                reply: None,
            });
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("pic_drag")));
        if let Some(t) = target {
            if let Some((_, r)) = self.album_drop.iter().rev().find(|(a, _)| *a == t) {
                painter.rect_stroke(r.expand(1.0), 4.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Outside);
            }
        }
        if let Some(p) = pos {
            let what = Self::count_text(ids.len(), albums.len());
            let text = match target {
                Some(t) if !ids.is_empty() && moves(t, self.here()) && in_albums => trf!("{what} → {} 로 옮기기   (Ctrl: 복사)", "{what} → move to {}   (Ctrl: copy)", self.album_label(t)),
                Some(t) if !ids.is_empty() && moves(t, self.cur_album()) => trf!("{what} → {} 로 옮기기", "{what} → move to {}", self.album_label(t)),
                Some(t) if !ids.is_empty() && in_albums && t.is_some() && self.here().is_some_and(|f| f != t) => trf!("{what} → {} 에 복사", "{what} → copy to {}", self.album_label(t)),
                Some(t) if !ids.is_empty() => trf!("{what} → {}   (Shift: 옮기기)", "{what} → {}   (Shift: move)", self.album_label(t)),
                Some(t) => format!("{what} → {}", self.album_label(t)),
                None if ids.is_empty() => trf!("{what}  (앨범 위에 놓기)", "{what}  (drop on an album)"),
                None => trf!("{what}  (앨범 위에 놓기 · 창 밖으로 끌면 파일로)", "{what}  (drop on an album · drag out of the window to copy files)"),
            };
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(14.0), Color32::WHITE);
            let r = Rect::from_min_size(p + vec2(16.0, 12.0), galley.size() + vec2(12.0, 8.0));
            painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(30, 34, 44, 235));
            painter.galley(r.min + vec2(6.0, 4.0), galley, Color32::WHITE);
        }
        ctx.request_repaint();
    }

    /// An album in the grid: a folder with its newest picture as cover.
    pub(super) fn draw_album_cell(&mut self, ui: &egui::Ui, rect: Rect, resp: &egui::Response, target: Option<u32>, img: Vec2, focused: bool) {
        let selected = target.is_some_and(|a| self.album_sel.contains(&a));
        let p = ui.painter().clone();
        let inner = rect.shrink(2.0);
        if selected {
            p.rect_filled(inner, 4.0, SEL);
            p.rect_stroke(inner, 4.0, Stroke::new(1.5, SEL_STROKE), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(inner, 4.0, CELL_HOVER);
        }
        if focused {
            p.rect_stroke(inner.shrink(1.0), 4.0, Stroke::new(1.0, FOCUS_STROKE), egui::StrokeKind::Inside);
        }
        let key = match target {
            Some(id) => format!("album_tile_{id}"),
            None => "album_tile_none".to_string(),
        };
        self.mark(&key, rect);
        let img_rect = Rect::from_min_size(pos2(rect.center().x - img.x / 2.0, rect.top() + 5.0), img);
        let col = if target.is_none() { Color32::from_rgb(70, 70, 76) } else { Color32::from_rgb(70, 92, 124) };
        // folder tab + body; the cover picture sits in the body
        p.rect_filled(Rect::from_min_size(img_rect.min + vec2(4.0, 0.0), vec2((img.x * 0.35).max(30.0), 12.0)), 3.0, col);
        let body = Rect::from_min_max(img_rect.min + vec2(0.0, 8.0), img_rect.max);
        p.rect_filled(body, 5.0, col);
        let pic = body.shrink(5.0);
        p.rect_filled(pic, 3.0, BG);
        if let Some(cid) = self.album_cover.get(&target).cloned() {
            if let Some(t) = self.thumb(&cid) {
                paint_fit(&p, pic, &t);
            }
        } else {
            p.text(pic.center(), egui::Align2::CENTER_CENTER, tr("비어 있음", "Empty"), egui::FontId::proportional(13.0 + UI_FONT_PLUS), DIM);
        }
        // name and counts on a dark strip at the bottom of the folder
        let n = match target {
            Some(a) => self.album_counts.get(&a).copied().unwrap_or(0),
            None => self.unfiled_n,
        };
        let kids = target.map(|a| self.db.album_children(a).len()).unwrap_or(0);
        let name = self.album_label(target);
        // a long name takes up to two lines: the strip grows up over the cover picture
        let mut job = egui::text::LayoutJob::simple(format!("📁 {name}"), egui::FontId::proportional(13.0 + UI_FONT_PLUS), TEXT, pic.width() - 8.0);
        job.wrap.max_rows = 2;
        job.wrap.break_anywhere = true;
        let g = ui.fonts_mut(|f| f.layout_job(job));
        let sub_h = 11.0 + UI_FONT_PLUS + 7.0;
        let strip_h = (g.size().y + 2.0 + sub_h).max(32.0 + UI_FONT_PLUS * 2.0).min(pic.height());
        let strip = Rect::from_min_max(pos2(pic.left(), pic.bottom() - strip_h), pic.max);
        p.rect_filled(strip, 0.0, Color32::from_black_alpha(185));
        self.mark(&format!("{key}_name"), Rect::from_min_size(strip.min + vec2(5.0, 2.0), g.size()));
        p.galley(strip.min + vec2(5.0, 2.0), g, TEXT);
        let sub = if kids > 0 { trf!("{n}장 · 앨범 {kids}", "{n} pictures · {kids} albums") } else { trf!("{n}장", "{n} pictures") };
        p.text(pos2(strip.left() + 6.0, strip.bottom() - 3.0), egui::Align2::LEFT_BOTTOM, sub, egui::FontId::proportional(11.0 + UI_FONT_PLUS), DIM);
        if resp.hovered() && self.marquee.is_none() && self.pic_drag.is_none() {
            let tip = match target {
                None => tr("무소속 — 어느 앨범에도 없는 그림\n더블클릭: 열기 · 그림을 여기 놓으면 모든 앨범에서 빠짐", "Unassigned — pictures in no album\nDouble-click: open · dropping pictures here removes them from all albums").to_string(),
                Some(id) => trf!("{0}\n더블클릭·Enter: 열기 · 클릭/Ctrl/Shift/끌기: 고르기\n고른 앨범을 다른 앨범에 끌어 놓기 또는 Ctrl+X → Ctrl+V: 옮기기\nF2 이름 · {1} 지우기 (그림은 그대로) · {2} 그림까지 · 오른쪽 클릭: 메뉴", "{0}\nDouble-click / Enter: open · click / Ctrl / Shift / drag: select\nDrag selected albums onto another album, or Ctrl+X → Ctrl+V: move\nF2 rename · {1} delete (pictures kept) · {2} with pictures · right-click: menu", self.db.album_path(id), self.del_keys().0, self.del_keys().1),
            };
            resp.clone().on_hover_text(tip);
        }
    }

    /// Right-click on empty space in the album view.
    pub(super) fn empty_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(200.0);
        if self.cur_album() == Some(None) {
            ui.label(egui::RichText::new(tr("무소속 안에는 앨범을 둘 수 없어 맨 위에 만듭니다", "Albums cannot go inside Unassigned; it will be created at the top level")).color(DIM).small());
        }
        let r = ui.button(tr("새 앨범…    Ctrl+N", "New album…    Ctrl+N"));
        self.mark("menu_new_album", r.rect);
        if r.clicked() {
            self.new_album_here();
            ui.close();
        }
        if ui.button(tr("붙여넣기    Ctrl+V", "Paste    Ctrl+V")).clicked() {
            self.paste_any();
            ui.close();
        }
        if ui.button(tr("모두 고르기    Ctrl+A", "Select all    Ctrl+A")).clicked() {
            self.on_key(egui::Key::A, Modifiers::CTRL);
            ui.close();
        }
    }

    /// Right-click menu for the selected albums.
    pub(super) fn album_menu(&mut self, ui: &mut egui::Ui) {
        let mut ids: Vec<u32> = self.album_sel.iter().copied().collect();
        ids.sort_unstable();
        if ids.is_empty() {
            ui.label(tr("고른 앨범 없음", "No album selected"));
            return;
        }
        ui.set_min_width(220.0);
        if let [id] = ids[..] {
            ui.label(egui::RichText::new(self.db.album_path(id)).color(DIM));
            ui.separator();
            if ui.button(tr("열기    Enter", "Open    Enter")).clicked() {
                self.open_album(Some(Some(id)));
                ui.close();
            }
            if ui.button(tr("하위 앨범 만들기…", "New sub-album…")).clicked() {
                self.new_album_under(id);
                ui.close();
            }
            if ui.button(tr("이름 바꾸기…    F2", "Rename…    F2")).clicked() {
                let name = self.album_label(Some(id));
                self.modal = Some(Modal::AlbumRename { id, new: name });
                ui.close();
            }
        } else {
            ui.label(egui::RichText::new(trf!("앨범 {}개", "{} albums", ids.len())).color(DIM));
            ui.separator();
        }
        if ui.button(tr("잘라 두기 (옮기기)    Ctrl+X", "Cut (to move)    Ctrl+X")).on_hover_text(tr("옮길 곳을 열고 Ctrl+V", "Open the destination and press Ctrl+V")).clicked() {
            self.album_copy(true);
            ui.close();
        }
        ui.separator();
        let (light_key, strong_key) = self.del_keys();
        if ui.button(trf!("앨범 지우기… (그림은 그대로)    {}", "Delete album… (pictures are kept)    {}", light_key)).clicked() {
            self.modal = Some(Modal::AlbumDelete { ids: ids.clone(), with_pics: false });
            ui.close();
        }
        if ui.button(trf!("앨범과 안의 그림까지 지우기… (그림은 imgr 휴지통)    {}", "Delete album with its pictures… (to the imgr Trash)    {}", strong_key)).clicked() {
            self.modal = Some(Modal::AlbumDelete { ids, with_pics: true });
            ui.close();
        }
    }

    pub(super) fn image_menu(&mut self, ui: &mut egui::Ui) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            ui.label(tr("고른 그림 없음", "No picture selected"));
            return;
        }
        let one = ids.len() == 1;
        let first = self.img(&ids[0]).cloned();
        ui.set_min_width(220.0);
        ui.label(egui::RichText::new(if one { first.as_ref().map(|i| i.name().to_string()).unwrap_or_default() } else { trf!("{}장", "{} pictures", ids.len()) }).color(DIM));
        ui.separator();
        if one && self.view != View::Viewer && ui.button(tr("크게 보기    Enter", "Viewer    Enter")).clicked() {
            let id = ids[0].clone();
            if let Some(&p) = self.pos_of.get(&id) {
                self.focus = Some(p);
            }
            let from = self.view;
            self.open_in_viewer(&id, None, from);
            ui.close();
        }
        if ids.len() >= 2 && ui.button(trf!("비교 ({}장)    C", "Compare ({})    C", ids.len().min(12))).clicked() {
            self.open_compare_ids(ids.clone());
            ui.close();
        }
        ui.menu_button(tr("점수", "Score"), |ui| {
            for n in 1..=6u8 {
                if ui.button(trf!("{n}점    {n}", "{n}    {n}")).clicked() {
                    self.set_score(n);
                    ui.close();
                }
            }
            if ui.button(tr("지우기    0", "Clear    0")).clicked() {
                self.set_score(0);
                ui.close();
            }
        });
        ui.menu_button(tr("해시태그", "Hashtag"), |ui| {
            let mut tags: Vec<String> = self.settings.recent_tags.clone();
            for (t, _) in &self.tag_counts {
                if tags.len() >= 20 {
                    break;
                }
                if !tags.contains(t) {
                    tags.push(t.clone());
                }
            }
            for t in tags {
                let all = ids.iter().filter_map(|id| self.img(id)).all(|i| i.tags.contains(&t));
                if ui.button(if all { trf!("✓ #{t}  (떼기)", "✓ #{t}  (remove)") } else { format!("#{t}") }).clicked() {
                    self.toggle_tag(&t);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tr("새 태그 입력…    T", "Enter new tag…    T")).clicked() {
                self.focus_tag_input = true;
                self.settings.show_right = true;
                ui.close();
            }
        });
        ui.menu_button(tr("앨범", "Album"), |ui| {
            let mut act: Option<(u32, bool)> = None;
            let mut list: Vec<(u32, String, usize)> = self.album_rows();
            // the album view's top: pictures can sit there and in albums at once
            list.insert(0, (0, tr("/ (맨 위)", "/ (top)").to_string(), 0));
            if list.len() == 1 {
                ui.label(egui::RichText::new(tr("아직 앨범이 없음", "No albums yet")).color(DIM));
            }
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (id, name, depth) in &list {
                    let all = ids.iter().filter_map(|x| self.img(x)).all(|i| i.albums.contains(id) || (*id == 0 && i.albums.is_empty()));
                    let label = format!("{}{}{}", "    ".repeat(*depth), if all { "✓ " } else { "" }, name);
                    if ui.button(label).on_hover_text(if all { tr("누르면 이 앨범에서 뺌", "Click to remove from this album") } else { tr("누르면 이 앨범에 넣음", "Click to add to this album") }).clicked() {
                        act = Some((*id, all));
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.button(tr("모든 앨범에서 빼기", "Remove from all albums")).clicked() {
                self.add_to_album(&ids, None);
                ui.close();
            }
            if ui.button(tr("새 앨범 만들어 넣기…", "Create new album and add…")).clicked() {
                // in the album view: the new album goes into the album shown
                let parent = match self.cur_album() {
                    Some(Some(p)) if self.view == View::Albums => p,
                    _ => 0,
                };
                self.modal = Some(Modal::AlbumNew { parent, new: String::new(), put: ids.clone() });
                ui.close();
            }
            if let Some((id, all)) = act {
                if all {
                    let name = self.album_label(Some(id));
                    self.edit(&ids, &trf!("앨범 '{name}'에서 빼기", "Remove from album '{name}'"), |im| im.albums.retain(|&a| a != id));
                    self.notice(trf!("{}장을 '{name}'에서 뺌 (Ctrl+Z 되돌리기)", "Removed {} pictures from '{name}' (Ctrl+Z to undo)", ids.len()), false);
                    self.refilter(true);
                } else {
                    self.add_to_album(&ids, Some(id));
                }
            }
        });
        ui.separator();
        if ui.button(if one { tr("이름 바꾸기…    F2", "Rename…    F2") } else { tr("이름 한꺼번에 바꾸기…    F2", "Batch rename…    F2") }).clicked() {
            self.open_rename();
            ui.close();
        }
        if ui.button(tr("내보내기 (업로드용)…    Ctrl+E", "Export (for upload)…    Ctrl+E")).clicked() {
            self.open_export();
            ui.close();
        }
        if ui.button(tr("파일 복사    Ctrl+C", "Copy file    Ctrl+C")).on_hover_text(tr("탐색기·브라우저 업로드 창에 붙여넣기", "Paste into Explorer or a browser upload dialog")).clicked() {
            self.copy_files();
            ui.close();
        }
        if ui.button(tr("경로 복사    Ctrl+Shift+C", "Copy path    Ctrl+Shift+C")).clicked() {
            self.copy_paths();
            ui.close();
        }
        if one && ui.button(tr("파일 열기", "Open file")).clicked() {
            if let Some(im) = &first {
                winshell::open_file(&self.img_path(im));
            }
            ui.close();
        }
        if ui.button(tr("탐색기에서 보기", "Show in Explorer")).clicked() {
            let paths = self.paths_of(&ids);
            winshell::reveal(&paths);
            ui.close();
        }
        if let Some(im) = &first {
            if one && !im.prompt.is_empty() && ui.button(tr("프롬프트 복사", "Copy prompt")).clicked() {
                ui.ctx().copy_text(im.prompt.clone());
                ui.close();
            }
            if let Some(s) = im.seed {
                if ui.button(tr("같은 시드 보기", "Show same seed")).clicked() {
                    self.set_query(format!("seed:{s} sort:seed"));
                    ui.close();
                }
            }
            if !im.preset.is_empty() && ui.button(trf!("같은 프리셋 보기 ({})", "Show same preset ({})", im.preset)).clicked() {
                self.set_query(format!("preset:{}", im.preset));
                ui.close();
            }
        }
        ui.separator();
        let all_trash = ids.iter().filter_map(|id| self.img(id)).all(|i| i.trash);
        // album view inside an album: Del takes the pictures out of this album only
        let here = match self.here() {
            Some(Some(x)) if self.view == View::Albums => Some(x),
            _ => None,
        };
        if let Some(x) = here {
            let name = self.album_label(Some(x));
            let light_key = self.del_keys().0;
            if ui.button(trf!("'{name}'에서 빼기    {light_key}", "Remove from '{name}'    {light_key}")).on_hover_text(tr("이 앨범에서만 뺌. 다른 앨범에 없는 그림은 imgr 휴지통으로", "Removes from this album only. Pictures in no other album go to the imgr Trash")).clicked() {
                self.take_out_here(&ids, x);
                ui.close();
            }
        }
        let del = if here.is_some() { format!("    {}", self.del_keys().1) } else { "    Del".to_string() };
        if ui.button(if all_trash { trf!("되살리기{del}", "Restore{del}") } else { trf!("imgr 휴지통으로{del}", "Move to imgr Trash{del}") }).clicked() {
            self.set_trash(!all_trash);
            ui.close();
        }
    }
}
