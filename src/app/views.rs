//! Big viewer, side-by-side compare, and the matrix (rows = seed, columns =
//! preset / checkpoint / LoRA set) for comparing test runs.

use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::*;
use crate::i18n::tr;

/// The 비교열 as shown, worked out once per change (not on every repaint).
pub(super) struct CmpCache {
    key: (u64, u64, usize, usize),
    rows: Vec<Vec<String>>,
    aspect: f32,
    /// preset, checkpoint, seed, LoRA differ somewhere
    d: [bool; 4],
}

pub(super) struct Matrix {
    generation: u64,
    key: String,
    pub cols: Vec<String>,
    /// (seed, ids per column)
    pub rows: Vec<(u64, Vec<Vec<String>>)>,
    /// (count, average score of scored ones)
    pub stats: Vec<(usize, f32)>,
}

fn col_value(im: &Img, key: &str) -> String {
    match key {
        "ckpt" => im.ckpt.clone(),
        "lora" => {
            if im.loras.is_empty() {
                tr("(LoRA 없음)", "(no LoRA)").into()
            } else {
                im.loras.join(" + ")
            }
        }
        _ => im.preset.clone(),
    }
}

impl App {
    // ------------------------------------------------------------ viewer
    pub(super) fn ui_viewer(&mut self, ui: &mut egui::Ui) {
        let Some(im) = self.viewer_img().cloned() else {
            if self.viewer_in_cmp() {
                // its row ran empty (trashed): back to the 비교열
                self.go_back();
            } else {
                self.view = View::Grid;
            }
            return;
        };
        let in_cmp = self.viewer_in_cmp();
        let cmp_row = if in_cmp { self.cmp_shown_row(self.cmp_focus.0) } else { Vec::new() };
        let full = ui.available_rect_before_wrap();
        let (rect, resp) = ui.allocate_exact_size(full.size(), Sense::click_and_drag());
        self.mark("viewer", rect);
        let tex = self.big_or_thumb(&im.id, BIG_PX);
        // read the neighbours ahead so ←/→ is instant
        let (cur, len) = if in_cmp { (Some(self.cmp_focus.1), cmp_row.len()) } else { (self.focus, self.shown.len()) };
        if let Some(f) = cur {
            for d in [1isize, -1, 2] {
                let p = f as isize + d;
                if p >= 0 && (p as usize) < len {
                    let id = if in_cmp { cmp_row[p as usize].clone() } else { self.db.images[self.shown[p as usize]].id.clone() };
                    if let Some(path) = self.img(&id).map(|i| self.img_path(i)) {
                        let _ = self.texture(&id, path, Kind::Big(BIG_PX));
                    }
                }
            }
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, BG);
        if let Some(t) = &tex {
            let px = vec2(im.w.max(1) as f32, im.h.max(1) as f32);
            let ppp = ui.ctx().pixels_per_point();
            zoom_input(ui, &resp, rect, px, &mut self.zp, ppp, false);
            paint_zoomed(&p, rect, t, &self.zp, px, ppp);
        }
        if resp.double_clicked() {
            self.go_back();
        }
        resp.context_menu(|ui| self.image_menu(ui));
        let pos = cur.unwrap_or(0) + 1;
        let tags = im.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ");
        let score = if im.score > 0 { trf!("{}점", "score {}", im.score) } else { tr("점수 없음", "no score").into() };
        let zoom = if self.zp.actual { tr("  ·  실제 크기", "  ·  actual size").to_string() } else if self.zp.zoom > 1.01 { format!("  ·  ×{:.1}", self.zp.zoom) } else { String::new() };
        let label = format!("{}   {}/{}   {}   {}{}", im.name(), pos, len, score, tags, zoom);
        // ` hides / shows the text lines at the top and bottom
        let text = self.settings.viewer_text;
        if text {
            overlay_label(&p, rect.left_top() + vec2(8.0, 8.0), &label);
        }
        // generation info too (fullscreen hides the right panel)
        let ck = im.ckpt.trim_end_matches(".safetensors");
        let info: Vec<String> = [
            (!im.preset.is_empty()).then(|| trf!("프리셋 {}", "preset {}", im.preset)),
            (!ck.is_empty()).then(|| ck.to_string()),
            im.seed.map(|s| trf!("시드 {s}", "seed {s}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        if text && !info.is_empty() {
            overlay_label(&p, rect.left_top() + vec2(8.0, 32.0), &info.join("  ·  "));
        }
        if im.score > 0 {
            let c = pos2(rect.right() - 30.0, rect.top() + 30.0);
            p.circle_filled(c, 18.0, score_color(im.score));
            p.text(c, egui::Align2::CENTER_CENTER, im.score.to_string(), egui::FontId::proportional(20.0), Color32::WHITE);
        }
        let adv = if self.settings.auto_advance { tr("1~6 점수(다음으로)", "1-6 score (then next)") } else { tr("1~6 점수", "1-6 score") };
        if text {
            overlay_label(&p, rect.left_bottom() + vec2(8.0, -26.0), &trf!("←/→ 넘기기 · 휠 확대 · 끌어서 이동 · R 맞춤 · Z 실제 크기 · {adv} · ` 글 숨기기 · F11 전체 화면 · Esc 뒤로", "←/→ prev/next · wheel zoom · drag to pan · R fit · Z actual size · {adv} · ` hide text · F11 fullscreen · Esc back"));
        }
    }

    // ------------------------------------------------------------ 비교열 (compare rows)
    pub(super) fn ui_compare(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        self.mark("compare", full);
        let key = (self.cmp_gen, self.shown_gen, self.db.images.len(), self.settings.cmp_rows.len());
        if self.cmp_cache.as_ref().is_none_or(|c| c.key != key) {
            let rows: Vec<Vec<String>> = (0..self.settings.cmp_rows.len()).map(|r| self.cmp_shown_row(r)).collect();
            let mut ar: Vec<f32> = rows.iter().flatten().take(400).filter_map(|id| self.img(id)).filter(|i| i.w > 0 && i.h > 0).map(|i| i.w as f32 / i.h as f32).collect();
            ar.sort_by(|a, b| a.total_cmp(b));
            let aspect = ar.get(ar.len() / 2).copied().unwrap_or(0.7).clamp(0.4, 2.0);
            let imgs: Vec<&Img> = rows.iter().flatten().filter_map(|id| self.img(id)).collect();
            let differs = |f: &dyn Fn(&Img) -> String| imgs.iter().map(|i| f(i)).collect::<std::collections::HashSet<_>>().len() > 1;
            let d = [
                differs(&|i| i.preset.clone()),
                differs(&|i| i.ckpt.clone()),
                differs(&|i| i.seed.map(|s| s.to_string()).unwrap_or_default()),
                differs(&|i| i.loras.join("+")),
            ];
            self.cmp_cache = Some(CmpCache { key, rows, aspect, d });
        }
        let (rows, aspect, [d_preset, d_ckpt, d_seed, d_lora]) = {
            let c = self.cmp_cache.as_ref().expect("just made");
            (c.rows.clone(), c.aspect, c.d)
        };
        let labels: Vec<String> = self.settings.cmp_rows.iter().map(|r| r.label.clone()).collect();
        let total_pics: usize = rows.iter().map(|r| r.len()).sum();
        // top bar: count, rows per screen, clear all
        let head_h = 32.0;
        let bar = Rect::from_min_size(full.min, vec2(full.width(), head_h));
        let body = Rect::from_min_max(pos2(full.left(), full.top() + head_h), full.max);
        let mut clear_all = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(8.0, 2.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(trf!("비교열 {}줄 · {}장", "Compare rows: {} · {} pictures", rows.len(), total_pics)).strong());
                ui.separator();
                ui.label(egui::RichText::new(tr("한 화면에", "Rows per screen")).color(DIM));
                for n in 1..=3u8 {
                    let r = ui.selectable_label(self.settings.cmp_per_screen == n, trf!("{n}줄", "{n}"));
                    self.mark(&format!("cmp_per_{n}"), r.rect);
                    if r.clicked() {
                        self.settings.cmp_per_screen = n;
                        self.cmp_scroll = true;
                    }
                }
                ui.separator();
                let r = ui.add_enabled(!rows.is_empty(), egui::Button::new(tr("모두 비우기", "Clear all")));
                self.mark("cmp_clear", r.rect);
                if r.clicked() {
                    clear_all = true;
                }
                ui.separator();
                let hint = tr("휠 위아래 · Shift+휠 좌우 · 끌어서 이동 · Ctrl+휠 확대(모두 같이) · ←→↑↓ 칸 고르기 · 1~6 그 칸 점수 · 더블클릭 크게 · R 맞춤 · Esc/C 뒤로", "wheel up/down · Shift+wheel sideways · drag to move · Ctrl+wheel zoom (all together) · ←→↑↓ select · 1-6 score that cell · double-click to enlarge · R fit · Esc / C back");
                ui.label(egui::RichText::new(tr("휠 위아래 · Shift+휠 좌우 · Ctrl+휠 확대 · 더블클릭 크게 · Esc 뒤로  ⓘ", "wheel up/down · Shift+wheel sideways · Ctrl+wheel zoom · double-click enlarge · Esc back  ⓘ")).color(DIM)).on_hover_text(hint);
            });
        });
        if clear_all {
            self.cmp_clear_all();
            return;
        }
        let p = ui.painter_at(body);
        p.rect_filled(body, 0.0, BG);
        if rows.is_empty() {
            p.text(body.center(), egui::Align2::CENTER_CENTER, tr("비교열이 비어 있습니다\n그림이나 앨범을 오른쪽 클릭 → 비교열에 추가 (Shift+C)\n한 번 넣은 것이 한 줄이 됩니다", "The compare rows are empty\nRight-click pictures or albums → Add to compare rows (Shift+C)\nEach add becomes one row"), egui::FontId::proportional(16.0 + UI_FONT_PLUS), DIM);
            return;
        }
        // one size for every cell, so the same column lines up from row to row
        let per = (self.settings.cmp_per_screen.clamp(1, 3) as usize).min(rows.len()) as f32;
        let row_head = 26.0;
        let lab_h = 22.0;
        let bar_w = 14.0; // room for a scroll bar
        let over_v = rows.len() as f32 > per;
        let view_h = body.height() - bar_w;
        let rh = (view_h / per).floor().max(170.0);
        let img_h = (rh - row_head - lab_h - 6.0).max(60.0);
        let max_n = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
        let fit_w = (body.width() - if over_v { bar_w } else { 0.0 }) / max_n as f32;
        let cw = fit_w.max(img_h * aspect + 6.0).max(110.0).floor();
        let total = vec2((cw * max_n as f32).max(body.width() - bar_w), rh * rows.len() as f32);
        // (label parts that differ over all the rows are bright, the same ones dim: d_*)
        let ppp = ui.ctx().pixels_per_point();
        // texture size by the cell (bigger when zoomed in), in steps so it is not reloaded all the time
        let want = (img_h.max(cw) * ppp * self.zp.zoom.clamp(1.0, 4.0)) as u32;
        let px = (want.div_ceil(512) * 512).clamp(512, BIG_PX);
        let mut op: Option<(usize, CmpOp)> = None;
        let mut take_scroll = std::mem::take(&mut self.cmp_scroll);
        let source = egui::scroll_area::ScrollSource { scroll_bar: true, mouse_wheel: true, ..egui::scroll_area::ScrollSource::NONE };
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            // solid scroll bars: always seen, so it is clear the rows go on (floating ones hide)
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            egui::ScrollArea::both().auto_shrink(false).id_salt("cmp_rows").scroll_source(source).show_viewport(ui, |ui, vp| {
                let (area, resp) = ui.allocate_exact_size(total, Sense::click_and_drag());
                let o = area.min;
                let r0 = (vp.min.y / rh).floor().max(0.0) as usize;
                let r1 = ((vp.max.y / rh).ceil().max(0.0) as usize).min(rows.len());
                let c0 = (vp.min.x / cw).floor().max(0.0) as usize;
                let c1 = ((vp.max.x / cw).ceil().max(0.0) as usize).min(max_n);
                let cell_of = |r: usize, c: usize| Rect::from_min_size(o + vec2(c as f32 * cw, r as f32 * rh + row_head), vec2(cw, rh - row_head)).shrink(2.0);
                let img_of = |cell: Rect| Rect::from_min_max(cell.min + vec2(0.0, lab_h), cell.max);
                if take_scroll {
                    let (fr, fc) = self.cmp_focus;
                    if fr < rows.len() {
                        ui.scroll_to_rect(cell_of(fr, fc).expand2(vec2(0.0, row_head)), None);
                    }
                    take_scroll = false;
                }
                let hover = ui.input(|i| i.pointer.hover_pos()).filter(|h| vp.translate(o.to_vec2()).contains(*h));
                let mut hover_cell = None;
                let mut cells = Vec::new();
                for (r, row) in rows.iter().enumerate().take(r1).skip(r0) {
                    for (c, id) in row.iter().enumerate().take(c1).skip(c0) {
                        let cell = cell_of(r, c);
                        if hover.is_some_and(|h| cell.contains(h)) {
                            hover_cell = Some((r, c));
                        }
                        let tex = self.big_or_thumb(id, px);
                        cells.push((r, c, cell, tex));
                    }
                }
                // zoom (Ctrl+wheel, all cells together) and pan (drag) on the picture under the mouse
                let zoomed = self.zp.zoom > 1.0001 || self.zp.actual;
                if let Some((r, c)) = hover_cell {
                    if let Some(im) = self.img(&rows[r][c]) {
                        let pxs = vec2(im.w.max(1) as f32, im.h.max(1) as f32);
                        zoom_input(ui, &resp, img_of(cell_of(r, c)), pxs, &mut self.zp, ppp, true);
                    }
                }
                if !zoomed && resp.dragged_by(egui::PointerButton::Primary) {
                    // not zoomed in: dragging moves the view like the scroll bars
                    ui.scroll_with_delta(resp.drag_delta());
                }
                if resp.clicked() || resp.secondary_clicked() || resp.double_clicked() {
                    if let Some(rc) = hover_cell {
                        self.cmp_focus = rc;
                    }
                }
                if resp.double_clicked() && hover_cell.is_some() {
                    self.cmp_open_viewer();
                }
                if hover_cell.is_some() || resp.context_menu_opened() {
                    resp.context_menu(|ui| self.image_menu(ui));
                }
                let p = ui.painter().clone();
                for (r, c, cell, tex) in &cells {
                    let Some(im) = self.img(&rows[*r][*c]) else { continue };
                    let img_r = img_of(*cell);
                    let pxs = vec2(im.w.max(1) as f32, im.h.max(1) as f32);
                    if let Some(t) = tex {
                        let pc = p.with_clip_rect(img_r.intersect(p.clip_rect()));
                        paint_zoomed(&pc, img_r, t, &self.zp, pxs, ppp);
                    }
                    let focused = (*r, *c) == self.cmp_focus;
                    p.rect_stroke(*cell, 2.0, Stroke::new(if focused { 2.0 } else { 1.0 }, if focused { ACCENT } else { Color32::from_gray(50) }), egui::StrokeKind::Inside);
                    let mut job = egui::text::LayoutJob::default();
                    let part = |job: &mut egui::text::LayoutJob, t: &str, hot: bool| {
                        if t.is_empty() {
                            return;
                        }
                        if !job.text.is_empty() {
                            job.append("  ·  ", 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.0 + UI_FONT_PLUS), DIM));
                        }
                        job.append(t, 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.0 + UI_FONT_PLUS), if hot { Color32::WHITE } else { DIM }));
                    };
                    part(&mut job, &im.preset, d_preset);
                    part(&mut job, &im.ckpt, d_ckpt);
                    if d_lora {
                        part(&mut job, &im.loras.join("+"), true);
                    }
                    part(&mut job, &im.seed.map(|s| format!("seed {s}")).unwrap_or_default(), d_seed);
                    if job.text.is_empty() {
                        part(&mut job, im.name(), true);
                    }
                    job.wrap.max_width = cell.width() - 10.0;
                    job.wrap.max_rows = 1;
                    let g = ui.fonts_mut(|f| f.layout_job(job));
                    p.with_clip_rect(cell.intersect(p.clip_rect())).galley(cell.left_top() + vec2(6.0, 4.0), g, TEXT);
                    if im.score > 0 {
                        let cc = pos2(img_r.left() + 14.0, img_r.top() + 14.0);
                        p.circle_filled(cc, 11.0, score_color(im.score));
                        p.text(cc, egui::Align2::CENTER_CENTER, im.score.to_string(), egui::FontId::proportional(14.0), Color32::WHITE);
                    }
                }
                // row heads stay at the left edge while scrolling sideways: ✕ ⤒ ↑ ↓ ⤓, then the label
                let vx = o.x + vp.min.x;
                for r in r0..r1 {
                    let y = o.y + r as f32 * rh;
                    let hr = Rect::from_min_size(pos2(vx, y), vec2(vp.width(), row_head));
                    p.rect_filled(hr, 0.0, PANEL);
                    let mut x = vx + 4.0;
                    let n = rows.len();
                    for (k, (txt, o2, tip, ok)) in [
                        ("✕", CmpOp::Remove, tr("이 줄 지우기 (그림은 그대로)", "Remove this row (the pictures stay)"), true),
                        ("⤒", CmpOp::Top, tr("맨 위로", "To the top"), r > 0),
                        ("↑", CmpOp::Up, tr("위로", "Up"), r > 0),
                        ("↓", CmpOp::Down, tr("아래로", "Down"), r + 1 < n),
                        ("⤓", CmpOp::Bottom, tr("맨 아래로", "To the bottom"), r + 1 < n),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let br = Rect::from_min_size(pos2(x, y + 2.0), vec2(24.0, row_head - 4.0));
                        x += 26.0;
                        let b = ui.add_enabled_ui(ok, |ui| ui.put(br, egui::Button::new(txt).small())).inner.on_hover_text(tip);
                        self.mark(&format!("cmp_row{r}_{k}"), br);
                        if b.clicked() && ok {
                            op = Some((r, o2));
                        }
                    }
                    let name = labels.get(r).cloned().unwrap_or_default();
                    let text = if rows[r].is_empty() {
                        trf!("{}. {name} · 안의 그림이 모두 휴지통에 있거나 사라짐 (되살리면 다시 보임)", "{}. {name} · all its pictures are in the Trash or gone (restore them to see them again)", r + 1)
                    } else {
                        trf!("{}. {name} · {}장", "{}. {name} · {} pictures", r + 1, rows[r].len())
                    };
                    let tc = if r == self.cmp_focus.0 { Color32::WHITE } else { DIM };
                    p.with_clip_rect(hr.intersect(p.clip_rect())).text(pos2(x + 6.0, y + row_head / 2.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.0 + UI_FONT_PLUS), tc);
                }
            });
        });
        self.cmp_scroll |= take_scroll;
        if let Some((r, o2)) = op {
            self.cmp_row_op(r, o2);
        }
    }

    // ------------------------------------------------------------ matrix
    fn matrix_data(&mut self) {
        let key = self.settings.matrix_col.clone();
        if let Some(m) = &self.mx_cache {
            if m.generation == self.shown_gen && m.key == key {
                return;
            }
        }
        let mut col_set: Vec<String> = Vec::new();
        let mut col_idx: HashMap<String, usize> = HashMap::new();
        let mut row_idx: HashMap<u64, usize> = HashMap::new();
        let mut rows: Vec<(u64, HashMap<usize, Vec<String>>)> = Vec::new();
        for &i in &self.shown {
            let im = &self.db.images[i];
            let Some(seed) = im.seed else { continue };
            let v = col_value(im, &key);
            if v.is_empty() {
                continue;
            }
            let c = *col_idx.entry(v.clone()).or_insert_with(|| {
                col_set.push(v.clone());
                col_set.len() - 1
            });
            let r = *row_idx.entry(seed).or_insert_with(|| {
                rows.push((seed, HashMap::new()));
                rows.len() - 1
            });
            rows[r].1.entry(c).or_default().push(im.id.clone());
        }
        // columns sorted by name
        let mut order: Vec<usize> = (0..col_set.len()).collect();
        order.sort_by(|&a, &b| col_set[a].to_lowercase().cmp(&col_set[b].to_lowercase()));
        let cols: Vec<String> = order.iter().map(|&c| col_set[c].clone()).collect();
        let multi = cols.len() > 1;
        let rows: Vec<(u64, Vec<Vec<String>>)> = rows
            .into_iter()
            .filter(|(_, m)| !multi || m.len() >= 2)
            .map(|(s, m)| (s, order.iter().map(|c| m.get(c).cloned().unwrap_or_default()).collect()))
            .collect();
        let mut stats = vec![(0usize, 0.0f32); cols.len()];
        for (c, st) in stats.iter_mut().enumerate() {
            let mut n = 0;
            let mut sum = 0u32;
            let mut scored = 0;
            for (_, cells) in &rows {
                for id in &cells[c] {
                    n += 1;
                    if let Some(i) = self.img(id) {
                        if i.score > 0 {
                            sum += i.score as u32;
                            scored += 1;
                        }
                    }
                }
            }
            *st = (n, if scored > 0 { sum as f32 / scored as f32 } else { 0.0 });
        }
        self.mx_cache = Some(Matrix { generation: self.shown_gen, key, cols, rows, stats });
    }

    fn visible_cols(&self) -> Vec<usize> {
        let Some(m) = &self.mx_cache else { return Vec::new() };
        (0..m.cols.len()).filter(|&c| !self.mx_hidden.contains(&m.cols[c])).collect()
    }

    pub(super) fn matrix_focus_id(&self) -> Option<String> {
        let (r, c) = self.mx_focus?;
        let m = self.mx_cache.as_ref()?;
        let vc = self.visible_cols();
        m.rows.get(r)?.1.get(*vc.get(c)?)?.first().cloned()
    }

    /// first picture of each visible column in the focused row (for C = compare)
    pub(super) fn matrix_row_ids(&self) -> Vec<String> {
        let Some(m) = &self.mx_cache else { return Vec::new() };
        let r = self.mx_focus.map(|f| f.0).unwrap_or(0);
        let Some(row) = m.rows.get(r) else { return Vec::new() };
        self.visible_cols().iter().filter_map(|&c| row.1.get(c).and_then(|v| v.first().cloned())).collect()
    }

    pub(super) fn matrix_move(&mut self, dr: isize, dc: isize) {
        let Some(m) = &self.mx_cache else { return };
        let nr = m.rows.len();
        let nc = self.visible_cols().len();
        if nr == 0 || nc == 0 {
            return;
        }
        let (r, c) = match self.mx_focus {
            // first key press: land on the first cell instead of skipping it
            None => (0, 0),
            Some((r, c)) => ((r as isize + dr).clamp(0, nr as isize - 1) as usize, (c as isize + dc).clamp(0, nc as isize - 1) as usize),
        };
        self.mx_focus = Some((r, c));
        if let Some(id) = self.matrix_focus_id() {
            self.sel.clear();
            self.sel.insert(id.clone());
            if let Some(&p) = self.pos_of.get(&id) {
                self.focus = Some(p);
            }
        }
    }

    pub(super) fn ui_matrix(&mut self, ui: &mut egui::Ui) {
        self.matrix_data();
        let mut new_key = None;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("표 비교", "Table compare")).strong());
            ui.label(egui::RichText::new(tr("행 = 시드 (같은 큐로 뽑은 그림)   열 =", "rows = seed (pictures from the same queue)   columns =")).color(DIM));
            for (k, label) in [("preset", tr("프리셋", "Preset")), ("ckpt", tr("체크포인트", "Checkpoint")), ("lora", tr("LoRA 조합", "LoRA combo"))] {
                if ui.selectable_label(self.settings.matrix_col == k, label).clicked() {
                    new_key = Some(k.to_string());
                }
            }
            if let Some(m) = &self.mx_cache {
                ui.separator();
                ui.label(egui::RichText::new(trf!("{}행 × {}열", "{} rows × {} columns", m.rows.len(), m.cols.len())).color(DIM));
            }
            if !self.mx_hidden.is_empty() && ui.button(trf!("숨긴 열 {}개 다시 보이기", "Show {} hidden columns again", self.mx_hidden.len())).clicked() {
                self.mx_hidden.clear();
            }
        });
        if let Some(k) = new_key {
            self.settings.matrix_col = k;
            self.mx_hidden.clear();
            self.mx_focus = None;
            self.matrix_data();
        }
        let Some(m) = self.mx_cache.take() else { return };
        if m.rows.is_empty() {
            ui.add_space(30.0);
            ui.label(egui::RichText::new(
                tr("같은 시드로 뽑은 그림이 열 두 개 이상 있어야 표가 생깁니다.\n예) 같은 큐를 animeA, photoB 두 프리셋으로 돌린 그림을 가져온 뒤, 검색으로 범위를 좁히고 M.\n지금 검색에 보이는 그림만 씁니다.", "The table needs pictures from the same seed in at least two columns.\nExample: import pictures from the same queue run with two presets (animeA, photoB), narrow the range by searching, then press M.\nOnly the pictures shown in the current search are used."),
            ).color(DIM));
            self.mx_cache = Some(m);
            return;
        }
        let vc: Vec<usize> = (0..m.cols.len()).filter(|&c| !self.mx_hidden.contains(&m.cols[c])).collect();
        let size = self.settings.thumb_size.round();
        let iw = (size * self.aspect).round();
        let cw = iw + 8.0;
        let ch = size + 8.0;
        let head_h = 40.0;
        let lab_w = 92.0;
        let total = vec2(lab_w + vc.len() as f32 * cw, head_h + m.rows.len() as f32 * ch);
        let mut clicked: Option<(usize, usize, bool, bool)> = None; // row, vis col, double, secondary
        let mut hide_col: Option<String> = None;
        let out = egui::ScrollArea::both().auto_shrink(false).id_salt("matrix").show_viewport(ui, |ui, viewport| {
            let (area, resp) = ui.allocate_exact_size(total, Sense::click());
            let origin = area.min;
            let p = ui.painter().clone();
            let r0 = ((viewport.min.y - head_h) / ch).floor().max(0.0) as usize;
            let r1 = (((viewport.max.y - head_h) / ch).ceil().max(0.0) as usize).min(m.rows.len());
            let c0 = ((viewport.min.x - lab_w) / cw).floor().max(0.0) as usize;
            let c1 = (((viewport.max.x - lab_w) / cw).ceil().max(0.0) as usize).min(vc.len());
            let hover = ui.input(|i| i.pointer.hover_pos());
            let mut hover_cell = None;
            for r in r0..r1 {
                for (vci, &c) in vc.iter().enumerate().take(c1).skip(c0) {
                    let cell = Rect::from_min_size(origin + vec2(lab_w + vci as f32 * cw, head_h + r as f32 * ch), vec2(cw, ch));
                    let ids = &m.rows[r].1[c];
                    if hover.is_some_and(|h| cell.contains(h)) {
                        hover_cell = Some((r, vci));
                    }
                    let Some(id) = ids.first() else {
                        p.rect_filled(cell.shrink(3.0), 3.0, Color32::from_rgb(28, 28, 31));
                        continue;
                    };
                    let selected = self.sel.contains(id);
                    let focused = self.mx_focus == Some((r, vci));
                    if selected {
                        p.rect_filled(cell.shrink(1.0), 3.0, SEL);
                    }
                    let img_r = cell.shrink(4.0);
                    let shown_r = match self.thumb(id) {
                        Some(t) => super::panels::paint_fit(&p, img_r, &t),
                        None => {
                            p.rect_filled(img_r.shrink(img_r.width() * 0.2), 3.0, Color32::from_rgb(36, 36, 40));
                            img_r
                        }
                    };
                    if focused {
                        p.rect_stroke(cell.shrink(1.0), 3.0, Stroke::new(1.5, FOCUS_STROKE), egui::StrokeKind::Inside);
                    }
                    if let Some(im) = self.img(id) {
                        if im.score > 0 {
                            let cc = pos2(shown_r.left() + 12.0, shown_r.top() + 12.0);
                            p.circle_filled(cc, 10.0, score_color(im.score));
                            p.text(cc, egui::Align2::CENTER_CENTER, im.score.to_string(), egui::FontId::proportional(13.0), Color32::WHITE);
                        }
                    }
                    if ids.len() > 1 {
                        let t = format!("+{}", ids.len() - 1);
                        let rr = Rect::from_min_size(pos2(shown_r.right() - 28.0, shown_r.top() + 4.0), vec2(24.0, 16.0));
                        p.rect_filled(rr, 3.0, TAG_BG);
                        p.text(rr.center(), egui::Align2::CENTER_CENTER, t, egui::FontId::proportional(11.0), TEXT);
                    }
                }
            }
            // sticky row labels (left) and column headers (top)
            let vx = origin.x + viewport.min.x;
            let vy = origin.y + viewport.min.y;
            for r in r0..r1 {
                let y = origin.y + head_h + r as f32 * ch;
                let lr = Rect::from_min_size(pos2(vx, y), vec2(lab_w, ch));
                p.rect_filled(lr, 0.0, PANEL);
                p.text(lr.left_top() + vec2(6.0, 6.0), egui::Align2::LEFT_TOP, format!("seed\n{}", m.rows[r].0), egui::FontId::proportional(12.0 + UI_FONT_PLUS), DIM);
            }
            let mut hover_head = None;
            for (vci, &c) in vc.iter().enumerate().take(c1).skip(c0) {
                let x = origin.x + lab_w + vci as f32 * cw;
                let hr = Rect::from_min_size(pos2(x, vy), vec2(cw, head_h));
                p.rect_filled(hr, 0.0, PANEL);
                let (cnt, avg) = m.stats[c];
                let mut job = egui::text::LayoutJob::default();
                job.append(&m.cols[c], 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.0 + UI_FONT_PLUS), Color32::WHITE));
                job.append(&trf!("\n{cnt}장{}", "\n{cnt} pictures{}", if avg > 0.0 { trf!(" · 평균 {avg:.1}점", " · avg {avg:.1}") } else { String::new() }), 0.0, egui::TextFormat::simple(egui::FontId::proportional(11.0 + UI_FONT_PLUS), DIM));
                job.wrap.max_width = cw - 8.0;
                job.wrap.max_rows = 2;
                let g = ui.fonts_mut(|f| f.layout_job(job));
                p.galley(hr.left_top() + vec2(4.0, 3.0), g, TEXT);
                if hover.is_some_and(|h| hr.contains(h)) {
                    hover_head = Some(c);
                }
            }
            p.rect_filled(Rect::from_min_size(pos2(vx, vy), vec2(lab_w, head_h)), 0.0, PANEL);
            if let Some(c) = hover_head {
                hover_cell = None;
                if resp.secondary_clicked() {
                    hide_col = Some(m.cols[c].clone());
                    self.mx_menu = false;
                }
                resp.clone().on_hover_text(tr("오른쪽 클릭: 이 열 숨기기", "Right-click: hide this column"));
            }
            if let Some((r, vci)) = hover_cell {
                if resp.double_clicked() {
                    clicked = Some((r, vci, true, false));
                } else if resp.clicked() {
                    clicked = Some((r, vci, false, false));
                } else if resp.secondary_clicked() {
                    clicked = Some((r, vci, false, true));
                    self.mx_menu = true;
                }
            } else if hover_head.is_none() && resp.secondary_clicked() {
                // empty space: no picture menu
                self.mx_menu = false;
            }
            resp
        });
        self.mark("matrix", out.inner_rect);
        let resp = out.inner;
        if let Some(c) = hide_col {
            self.mx_hidden.insert(c);
            self.mx_focus = None;
        }
        self.mx_cache = Some(m);
        if let Some((r, vci, dbl, sec)) = clicked {
            let ctrl = ui.input(|i| i.modifiers.ctrl);
            self.mx_focus = Some((r, vci));
            if let Some(id) = self.matrix_focus_id() {
                if ctrl && !sec {
                    if !self.sel.remove(&id) {
                        self.sel.insert(id.clone());
                    }
                } else if !(sec && self.sel.contains(&id)) {
                    self.sel.clear();
                    self.sel.insert(id.clone());
                }
                if let Some(&p) = self.pos_of.get(&id) {
                    self.focus = Some(p);
                }
                if dbl {
                    self.open_in_viewer(&id, None, View::Matrix);
                }
            }
        }
        if self.mx_menu {
            resp.context_menu(|ui| self.image_menu(ui));
        }
    }
}

pub(super) fn overlay_label(p: &egui::Painter, at: Pos2, text: &str) {
    let g = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0 + UI_FONT_PLUS), Color32::from_gray(220));
    let r = Rect::from_min_size(at, g.size()).expand(5.0);
    p.rect_filled(r, 4.0, Color32::from_black_alpha(150));
    p.galley(at, g, Color32::WHITE);
}

/// Image rect on screen for the current zoom/pan (zoom 1 = fit, or 1:1 pixels).
fn zoomed_rect(rect: Rect, px: Vec2, zp: &ZoomPan, ppp: f32) -> Rect {
    let base = if zp.actual { 1.0 / ppp } else { (rect.width() / px.x).min(rect.height() / px.y) };
    let size = px * base * zp.zoom.max(1.0);
    let min = rect.center() - vec2(zp.center.x * size.x, zp.center.y * size.y);
    Rect::from_min_size(min, size)
}

fn paint_zoomed(p: &egui::Painter, rect: Rect, t: &egui::TextureHandle, zp: &ZoomPan, px: Vec2, ppp: f32) {
    let r = zoomed_rect(rect, px, zp, ppp);
    p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
}

/// Wheel zoom and drag pan. `ctrl_zoom`: zoom with Ctrl+wheel (the wheel scrolls there).
fn zoom_input(ui: &egui::Ui, resp: &egui::Response, rect: Rect, px: Vec2, zp: &mut ZoomPan, ppp: f32, ctrl_zoom: bool) {
    if zp.zoom < 1.0 {
        zp.zoom = 1.0;
    }
    let img_r = zoomed_rect(rect, px, zp, ppp);
    let factor = if ctrl_zoom {
        ui.input(|i| i.zoom_delta())
    } else if ui.input(|i| i.modifiers.ctrl) {
        1.0
    } else {
        1.0 + ui.input(|i| i.smooth_scroll_delta.y) * 0.0025
    };
    if resp.hovered() {
        if (factor - 1.0).abs() > 1e-4 {
            let old = zp.zoom;
            let new = (old * factor).clamp(1.0, 40.0);
            if let Some(m) = ui.input(|i| i.pointer.hover_pos()) {
                // keep the image point under the mouse where it is
                let fx = (m.x - img_r.left()) / img_r.width();
                let fy = (m.y - img_r.top()) / img_r.height();
                let k = old / new;
                zp.center.x = fx + (zp.center.x - fx) * k;
                zp.center.y = fy + (zp.center.y - fy) * k;
            }
            zp.zoom = new;
        }
    }
    if resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        zp.center.x -= d.x / img_r.width();
        zp.center.y -= d.y / img_r.height();
    }
    let fits = img_r.width() <= rect.width() + 0.5 && img_r.height() <= rect.height() + 0.5;
    if zp.zoom <= 1.0001 && !zp.actual || fits {
        zp.center = vec2(0.5, 0.5);
    } else {
        zp.center.x = zp.center.x.clamp(0.0, 1.0);
        zp.center.y = zp.center.y.clamp(0.0, 1.0);
    }
}
