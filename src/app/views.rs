//! Big viewer, side-by-side compare, and the matrix (rows = seed, columns =
//! preset / checkpoint / LoRA set) for comparing test runs.

use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::*;
use crate::i18n::tr;

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
        let Some(im) = self.focus_img().cloned() else {
            self.view = View::Grid;
            return;
        };
        let full = ui.available_rect_before_wrap();
        let (rect, resp) = ui.allocate_exact_size(full.size(), Sense::click_and_drag());
        self.mark("viewer", rect);
        let tex = self.big_or_thumb(&im.id, BIG_PX);
        // read the neighbours ahead so ←/→ is instant
        if let Some(f) = self.focus {
            for d in [1isize, -1, 2] {
                let p = f as isize + d;
                if p >= 0 && (p as usize) < self.shown.len() {
                    let id = self.db.images[self.shown[p as usize]].id.clone();
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
            zoom_input(ui, &resp, rect, px, &mut self.zp, ppp);
            paint_zoomed(&p, rect, t, &self.zp, px, ppp);
        }
        if resp.double_clicked() {
            self.go_back();
        }
        resp.context_menu(|ui| self.image_menu(ui));
        let pos = self.focus.unwrap_or(0) + 1;
        let tags = im.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ");
        let score = if im.score > 0 { trf!("{}점", "score {}", im.score) } else { tr("점수 없음", "no score").into() };
        let zoom = if self.zp.actual { tr("  ·  실제 크기", "  ·  actual size").to_string() } else if self.zp.zoom > 1.01 { format!("  ·  ×{:.1}", self.zp.zoom) } else { String::new() };
        let label = format!("{}   {}/{}   {}   {}{}", im.name(), pos, self.shown.len(), score, tags, zoom);
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

    // ------------------------------------------------------------ compare
    pub(super) fn ui_compare(&mut self, ui: &mut egui::Ui) {
        let ids: Vec<String> = self.cmp_ids.iter().filter(|id| self.id_index.contains_key(*id)).cloned().collect();
        if ids.len() < 2 {
            self.go_back();
            return;
        }
        if ids.len() != self.cmp_ids.len() {
            self.cmp_ids = ids.clone();
            self.cmp_focus = self.cmp_focus.min(ids.len() - 1);
        }
        let full = ui.available_rect_before_wrap();
        let n = ids.len();
        let rows = if n > 8 { 3 } else if n > 4 { 2 } else { 1 };
        let cols = n.div_ceil(rows);
        let cw = full.width() / cols as f32;
        let ch = full.height() / rows as f32;
        let (all, resp) = ui.allocate_exact_size(full.size(), Sense::click_and_drag());
        self.mark("compare", all);
        let imgs: Vec<Img> = ids.iter().filter_map(|id| self.img(id).cloned()).collect();
        // which label parts differ between the pictures
        let differs = |f: &dyn Fn(&Img) -> String| imgs.iter().map(f).collect::<std::collections::HashSet<_>>().len() > 1;
        let d_preset = differs(&|i| i.preset.clone());
        let d_ckpt = differs(&|i| i.ckpt.clone());
        let d_seed = differs(&|i| i.seed.map(|s| s.to_string()).unwrap_or_default());
        let d_lora = differs(&|i| i.loras.join("+"));
        let px = if n <= 2 { BIG_PX } else { 1600 };
        let mut cells = Vec::new();
        for (k, im) in imgs.iter().enumerate() {
            let r = (k / cols) as f32;
            let c = (k % cols) as f32;
            let cell = Rect::from_min_size(pos2(full.left() + c * cw, full.top() + r * ch), vec2(cw, ch)).shrink(2.0);
            let tex = self.big_or_thumb(&im.id, px);
            cells.push((k, cell, tex));
        }
        let hover = ui.input(|i| i.pointer.hover_pos());
        let hover_cell = hover.and_then(|h| cells.iter().find(|c| c.1.contains(h)).map(|c| (c.0, c.1)));
        let ppp = ui.ctx().pixels_per_point();
        if let Some((k, cell)) = hover_cell {
            let im = &imgs[k];
            let img_r = Rect::from_min_max(cell.min + vec2(0.0, 22.0), cell.max);
            zoom_input(ui, &resp, img_r, vec2(im.w.max(1) as f32, im.h.max(1) as f32), &mut self.zp, ppp);
        }
        if resp.clicked() || resp.secondary_clicked() {
            if let Some((k, _)) = hover_cell {
                self.cmp_focus = k;
            }
        }
        if resp.double_clicked() {
            if let Some((k, _)) = hover_cell {
                let id = imgs[k].id.clone();
                self.open_in_viewer(&id, None, View::Compare);
            }
        }
        resp.context_menu(|ui| self.image_menu(ui));
        let p = ui.painter_at(all);
        p.rect_filled(all, 0.0, BG);
        for (k, cell, tex) in &cells {
            let im = &imgs[*k];
            let img_r = Rect::from_min_max(cell.min + vec2(0.0, 22.0), cell.max);
            let pc = ui.painter_at(img_r);
            if let Some(t) = tex {
                paint_zoomed(&pc, img_r, t, &self.zp, vec2(im.w.max(1) as f32, im.h.max(1) as f32), ppp);
            }
            let focused = *k == self.cmp_focus;
            p.rect_stroke(*cell, 2.0, Stroke::new(if focused { 2.0 } else { 1.0 }, if focused { ACCENT } else { Color32::from_gray(50) }), egui::StrokeKind::Inside);
            // label: parts that differ are bright, the same ones dim
            let mut job = egui::text::LayoutJob::default();
            let part = |job: &mut egui::text::LayoutJob, t: &str, hot: bool| {
                if t.is_empty() {
                    return;
                }
                if !job.text.is_empty() {
                    job.append("  ·  ", 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.0 + UI_FONT_PLUS), DIM));
                }
                let color = if hot { Color32::WHITE } else { DIM };
                job.append(t, 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.0 + UI_FONT_PLUS), color));
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
            part(&mut job, &if im.score > 0 { trf!("{}점", "{}", im.score) } else { "-".into() }, true);
            job.wrap.max_width = cell.width() - 10.0;
            job.wrap.max_rows = 1;
            let g = ui.fonts_mut(|f| f.layout_job(job));
            p.galley(cell.left_top() + vec2(6.0, 4.0), g, TEXT);
            if im.score > 0 {
                let c = pos2(img_r.left() + 14.0, img_r.top() + 14.0);
                p.circle_filled(c, 11.0, score_color(im.score));
                p.text(c, egui::Align2::CENTER_CENTER, im.score.to_string(), egui::FontId::proportional(14.0), Color32::WHITE);
            }
        }
        overlay_label(&p, all.left_bottom() + vec2(8.0, -26.0), tr("휠 확대(모두 같이) · 끌어서 이동 · 클릭/←→ 칸 고르기 · 1~6 그 칸 점수 · 더블클릭 크게 · R 맞춤 · Esc/C 뒤로", "wheel zoom (all together) · drag to pan · click / ←→ select cell · 1-6 score that cell · double-click to enlarge · R fit · Esc / C back"));
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

fn zoom_input(ui: &egui::Ui, resp: &egui::Response, rect: Rect, px: Vec2, zp: &mut ZoomPan, ppp: f32) {
    if zp.zoom < 1.0 {
        zp.zoom = 1.0;
    }
    let img_r = zoomed_rect(rect, px, zp, ppp);
    if resp.hovered() && !ui.input(|i| i.modifiers.ctrl) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            let old = zp.zoom;
            let new = (old * (1.0 + dy * 0.0025)).clamp(1.0, 40.0);
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
