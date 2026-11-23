//! Library screen: cards for characters / outfits / expressions / poses /
//! presets with their example pictures, and the detail panel on the right.

use eframe::egui::{self, Color32, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::panels::paint_fit;
use super::*;
use crate::library::Kind as LK;
use crate::i18n::tr;

/// Display name of a preset-summary row (the key itself stays Korean data).
fn sum_key(k: &str) -> &str {
    match k {
        "오류" => tr("오류", "Error"),
        "메모" => tr("메모", "Note"),
        "체크포인트" => tr("체크포인트", "Checkpoint"),
        "얼굴 다시그리기" => tr("얼굴 다시그리기", "Face redraw"),
        "눈" => tr("눈", "Eyes"),
        "손" => tr("손", "Hands"),
        "추가 태그" => tr("추가 태그", "Extra tags"),
        _ => k,
    }
}

impl App {
    fn lib_kind_filter(&self) -> Option<LK> {
        LK::from_key(&self.settings.lib_kind)
    }

    pub(super) fn ui_library(&mut self, ui: &mut egui::Ui) {
        self.ensure_lib_examples();
        let kf = self.lib_kind_filter();
        let mut set_kind: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(tr("사전", "Library")).strong().size(16.0));
            if ui.selectable_label(kf.is_none(), crate::trf!("전체 {}", "All {}", self.lib.len())).clicked() {
                set_kind = Some(String::new());
            }
            for k in LK::ALL {
                let n = self.lib.iter().filter(|e| e.kind == k).count();
                let exists = self.lib_folders.iter().any(|f| f.kind == k && f.exists);
                let label = if exists { format!("{} {n}", k.label()) } else { format!("{} -", k.label()) };
                if ui.selectable_label(kf == Some(k), label).clicked() {
                    set_kind = Some(k.key().to_string());
                }
            }
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.lib_search).hint_text(tr("이름·태그로 찾기", "Search name / tags")).desired_width(160.0));
            if ui.button(tr("＋ 새 항목", "＋ New item")).on_hover_text(tr("캐릭터·의상·표정·포즈 txt 만들기", "Create a txt for a character / outfit / expression / pose")).clicked() {
                let kind = kf.filter(|k| !k.is_preset()).unwrap_or(LK::Char);
                self.modal = Some(Modal::NewEntry { kind, name: library::next_name(&self.lib, kind), text: String::new(), from: None, error: String::new() });
            }
            if ui.button("⟳").on_hover_text(tr("파일 다시 읽기", "Reload files")).clicked() {
                self.reload_library();
            }
            if ui.button(tr("폴더…", "Folder…")).on_hover_text(crate::trf!("_gen 폴더: {}", "_gen folder: {}", self.settings.gen_dir)).clicked() {
                self.modal = Some(Modal::GenDir { dir: self.settings.gen_dir.clone() });
            }
        });
        if let Some(k) = set_kind {
            self.settings.lib_kind = k;
        }
        let kf = self.lib_kind_filter();
        ui.add_space(4.0);
        if let Some(k) = kf {
            if let Some(f) = self.lib_folders.iter().find(|f| f.kind == k) {
                if !f.exists {
                    ui.label(egui::RichText::new(crate::trf!("{} 폴더가 아직 없습니다: {}\n'＋ 새 항목'으로 만들면 폴더도 생깁니다.", "The {} folder does not exist yet: {}\nCreating one with '＋ New item' also creates the folder.", k.label(), f.path.display())).color(DIM));
                    return;
                }
            }
        }
        if !self.gen_dir().is_dir() {
            ui.label(egui::RichText::new(crate::trf!("_gen 폴더를 찾을 수 없습니다: {}\n위 '폴더…'에서 고르세요.", "Cannot find the _gen folder: {}\nPick it with 'Folder…' above.", self.settings.gen_dir)).color(ERR));
            return;
        }
        let s = self.lib_search.trim().to_lowercase();
        let list: Vec<usize> = (0..self.lib.len())
            .filter(|&i| {
                let e = &self.lib[i];
                kf.is_none_or(|k| e.kind == k)
                    && (s.is_empty() || e.name.to_lowercase().contains(&s) || e.text.to_lowercase().contains(&s) || self.db.lib.get(&e.key).is_some_and(|m| m.note.to_lowercase().contains(&s)))
            })
            .collect();
        if list.is_empty() {
            ui.label(egui::RichText::new(tr("항목이 없습니다", "No items")).color(DIM));
            return;
        }
        let size = self.settings.thumb_size.round().max(120.0);
        let card = vec2((size * 0.78).round() + 12.0, size + 64.0);
        let cols = ((ui.available_width() - 8.0) / card.x).floor().max(1.0) as usize;
        let rows = list.len().div_ceil(cols);
        let mut clicked: Option<(String, bool)> = None;
        let mut menu: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink(false).id_salt("lib_cards").show_rows(ui, card.y + 6.0, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    for c in 0..cols {
                        let k = r * cols + c;
                        let Some(&ei) = list.get(k) else { break };
                        let e = self.lib[ei].clone();
                        let (rect, resp) = ui.allocate_exact_size(card, Sense::click());
                        let sel = self.lib_sel.as_deref() == Some(e.key.as_str());
                        let p = ui.painter().clone();
                        p.rect_filled(rect.shrink(2.0), 6.0, if sel { SEL } else if resp.hovered() { CELL_HOVER } else { Color32::from_rgb(30, 30, 34) });
                        if sel {
                            p.rect_stroke(rect.shrink(2.0), 6.0, Stroke::new(1.5, SEL_STROKE), egui::StrokeKind::Inside);
                        }
                        let img_r = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(card.x - 12.0, size - 6.0));
                        let ex = self.lib_examples.get(&e.key).cloned().unwrap_or_default();
                        match ex.first().and_then(|id| self.thumb(id)) {
                            Some(t) => {
                                paint_fit(&p, img_r, &t);
                            }
                            None => {
                                p.rect_filled(img_r.shrink(10.0), 4.0, Color32::from_rgb(38, 38, 42));
                                p.text(img_r.center(), egui::Align2::CENTER_CENTER, tr("예시 없음", "No examples"), egui::FontId::proportional(13.0 + UI_FONT_PLUS), DIM);
                            }
                        }
                        let ty = img_r.bottom() + 4.0;
                        let mut job = egui::text::LayoutJob::simple(e.name.clone(), egui::FontId::proportional(13.0 + UI_FONT_PLUS), Color32::WHITE, card.x - 12.0);
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        let g = ui.fonts_mut(|f| f.layout_job(job));
                        p.galley(pos2(rect.left() + 6.0, ty), g, TEXT);
                        let sub = crate::trf!("{} · 예시 {}", "{} · {} examples", e.kind.label(), ex.len());
                        p.text(pos2(rect.left() + 6.0, ty + 18.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(11.0 + UI_FONT_PLUS), ACCENT);
                        let line = if e.kind.is_preset() {
                            e.summary.iter().find(|(k, _)| k == "LoRA" || k == "체크포인트").map(|x| x.1.clone()).unwrap_or_default()
                        } else {
                            e.tags.join(", ")
                        };
                        let mut job = egui::text::LayoutJob::simple(line, egui::FontId::proportional(11.0 + UI_FONT_PLUS), DIM, card.x - 12.0);
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        let g = ui.fonts_mut(|f| f.layout_job(job));
                        p.galley(pos2(rect.left() + 6.0, ty + 33.0), g, DIM);
                        if resp.double_clicked() {
                            clicked = Some((e.key.clone(), true));
                        } else if resp.clicked() || resp.secondary_clicked() {
                            clicked = Some((e.key.clone(), false));
                        }
                        if resp.secondary_clicked() {
                            menu = Some(e.key.clone());
                        }
                        let tags = e.text.clone();
                        let key = e.key.clone();
                        resp.context_menu(|ui| {
                            if ui.button(tr("예시 그림 모두 보기", "Show all example pictures")).clicked() {
                                self.lib_sel = Some(key.clone());
                                self.lib_open_examples();
                                ui.close();
                            }
                            if !e.kind.is_preset() && ui.button(tr("태그 복사", "Copy tags")).clicked() {
                                ui.ctx().copy_text(tags.trim().to_string());
                                ui.close();
                            }
                            if ui.button(tr("파일 열기", "Open file")).clicked() {
                                winshell::open_file(&e.file);
                                ui.close();
                            }
                            if ui.button(tr("탐색기에서 보기", "Show in Explorer")).clicked() {
                                winshell::reveal(std::slice::from_ref(&e.file));
                                ui.close();
                            }
                            let sel_ids = self.selected_ids();
                            if !sel_ids.is_empty() && ui.button(crate::trf!("고른 그림 {}장을 예시로 고정", "Pin {} selected pictures as examples", sel_ids.len())).clicked() {
                                self.lib_pin(&key, &sel_ids, false);
                                ui.close();
                            }
                        });
                    }
                });
                ui.add_space(6.0);
            }
        });
        let _ = menu;
        if let Some((k, dbl)) = clicked {
            self.lib_sel = Some(k);
            if dbl {
                self.lib_open_examples();
            }
        }
    }

    /// Right panel in the library view.
    pub(super) fn ui_lib_detail(&mut self, ui: &mut egui::Ui) {
        self.ensure_lib_examples();
        let Some(key) = self.lib_sel.clone() else {
            ui.add_space(20.0);
            ui.label(egui::RichText::new(tr("왼쪽 카드를 고르면 태그·설정·예시 그림이 나옵니다.\n더블클릭 = 예시 그림을 목록으로 보기", "Pick a card on the left to see tags, settings and example pictures.\nDouble-click = show example pictures as a list")).color(DIM));
            return;
        };
        let Some(e) = self.lib.iter().find(|x| x.key == key).cloned() else {
            ui.label(egui::RichText::new(crate::trf!("'{key}' 항목이 없습니다 (파일이 지워졌거나 이름이 바뀜)", "Item '{key}' not found (the file was deleted or renamed)")).color(ERR));
            return;
        };
        if self.lib_buf_key != key {
            // keep unsaved tag edits of the card we leave as a draft
            if let Some(old) = self.lib.iter().find(|x| x.key == self.lib_buf_key) {
                if self.lib_text != old.text {
                    self.lib_drafts.insert(old.key.clone(), self.lib_text.clone());
                } else {
                    self.lib_drafts.remove(&old.key);
                }
            }
            self.lib_buf_key = key.clone();
            self.lib_text = self.lib_drafts.get(&key).cloned().unwrap_or_else(|| e.text.clone());
            self.lib_note = self.db.lib.get(&key).map(|m| m.note.clone()).unwrap_or_default();
            self.lib_readme = if e.kind.is_preset() { library::readme_lines(&self.gen_dir(), &e.name) } else { Vec::new() };
        }
        let ex = self.lib_examples.get(&key).cloned().unwrap_or_default();
        egui::ScrollArea::vertical().auto_shrink(false).id_salt("lib_detail").show(ui, |ui| {
            ui.label(egui::RichText::new(&e.name).strong().size(16.0));
            ui.label(egui::RichText::new(crate::trf!("{} · 예시 {}장", "{} · {} examples", e.kind.label(), ex.len())).color(ACCENT));
            let r = ui.add(egui::Label::new(egui::RichText::new(e.file.display().to_string()).color(DIM).small()).sense(Sense::click())).on_hover_text(tr("클릭: 탐색기에서 보기", "Click: show in Explorer"));
            if r.clicked() {
                winshell::reveal(std::slice::from_ref(&e.file));
            }
            ui.horizontal_wrapped(|ui| {
                if !e.kind.is_preset() && ui.button(tr("태그 복사", "Copy tags")).clicked() {
                    ui.ctx().copy_text(e.text.trim().to_string());
                    self.notice(tr("태그를 복사함", "Tags copied"), false);
                }
                if ui.button(tr("파일 열기", "Open file")).clicked() {
                    winshell::open_file(&e.file);
                }
                if ui.add_enabled(!ex.is_empty(), egui::Button::new(tr("예시 모두 보기", "Show all examples"))).on_hover_text(tr("목록에서 lib: 검색으로 보기 (Enter)", "Show in the list with a lib: search (Enter)")).clicked() {
                    self.lib_open_examples();
                }
                if ui.add_enabled(ex.len() >= 2, egui::Button::new(tr("예시 비교", "Compare examples"))).clicked() {
                    let q = format!("lib:{key}");
                    self.set_query(q);
                    let ids: Vec<String> = ex.iter().take(8).cloned().collect();
                    self.open_compare_ids(ids);
                }
                let sel_ids: Vec<String> = self.sel.iter().cloned().collect();
                if !sel_ids.is_empty() && ui.button(crate::trf!("고른 그림 {}장 고정", "Pin {} selected pictures", sel_ids.len())).on_hover_text(tr("목록에서 고른 그림을 이 항목의 예시로", "Use the pictures selected in the list as examples of this item")).clicked() {
                    self.lib_pin(&key, &sel_ids, false);
                }
            });
            ui.add_space(4.0);
            if e.kind.is_preset() {
                egui::Grid::new("preset_sum").num_columns(2).spacing(vec2(8.0, 3.0)).show(ui, |ui| {
                    for (k, v) in &e.summary {
                        ui.label(egui::RichText::new(sum_key(k)).color(DIM));
                        ui.add(egui::Label::new(v).wrap());
                        ui.end_row();
                    }
                });
                if !self.lib_readme.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("presets\\readme.txt").color(DIM).small());
                    for l in &self.lib_readme {
                        ui.add(egui::Label::new(l).wrap());
                    }
                }
                egui::CollapsingHeader::new(tr("json 전체 (읽기만)", "Full json (read only)")).id_salt("preset_json").default_open(false).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&e.text).monospace().small()).selectable(true).wrap());
                });
            } else {
                ui.label(egui::RichText::new(crate::trf!("태그 ({}개, 예시 찾기에 씀)", "Tags ({}, used to find examples)", e.tags.len())).color(DIM));
                let r = ui.add(egui::TextEdit::multiline(&mut self.lib_text).desired_width(f32::INFINITY).desired_rows(4));
                self.mark("lib_text", r.rect);
                let changed = self.lib_text != e.text;
                if changed {
                    ui.horizontal(|ui| {
                        if ui.button(tr("저장", "Save")).on_hover_text(tr("파일에 씀. 이전 내용은 imgr\\data\\lib_backup 에 남음", "Writes the file. The old content stays in imgr\\data\\lib_backup")).clicked() {
                            match library::write_entry(&e.file, &self.lib_text) {
                                Ok(()) => {
                                    self.lib_drafts.remove(&key);
                                    self.notice(crate::trf!("'{}' 저장함 (이전 내용은 data\\lib_backup)", "Saved '{}' (old content is in data\\lib_backup)", e.name), false);
                                    self.reload_library();
                                    self.lib_sel = Some(key.clone());
                                }
                                Err(er) => self.notice(crate::trf!("저장 실패: {er}", "Save failed: {er}"), true),
                            }
                        }
                        if ui.button(tr("되돌리기", "Revert")).clicked() {
                            self.lib_text = e.text.clone();
                            self.lib_drafts.remove(&key);
                        }
                        ui.label(egui::RichText::new(tr("저장 안 함", "Not saved")).color(ACCENT).small());
                        ui.label(egui::RichText::new(tr("생성기가 읽는 파일입니다", "This file is read by the generator")).color(ERR).small());
                    });
                }
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr("메모 (imgr에만 저장)", "Note (stored only in imgr)")).color(DIM));
            let r = ui.add(egui::TextEdit::multiline(&mut self.lib_note).hint_text(tr("예) 눈 색 잘 나옴 / 손 망가지기 쉬움 / photoB와 잘 맞음", "e.g. eye color comes out well / hands break easily / goes well with photoB")).desired_width(f32::INFINITY).desired_rows(2));
            if r.changed() {
                let note = self.lib_note.clone();
                self.lib_meta_mut(&key).note = note;
                self.dirty = true;
            }
            ui.add_space(6.0);
            ui.separator();
            let pins: Vec<String> = self.db.lib.get(&key).map(|m| m.pins.clone()).unwrap_or_default();
            let cover = self.db.lib.get(&key).and_then(|m| m.cover.clone());
            let hidden_n = self.db.lib.get(&key).map(|m| m.hidden.len()).unwrap_or(0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(crate::trf!("예시 {}장", "{} examples", ex.len())).strong());
                ui.label(egui::RichText::new(tr("(점수 높은 순 · 📌 고정 · 오른쪽 클릭 메뉴)", "(highest score first · 📌 pinned · right-click menu)")).color(DIM).small());
            });
            if hidden_n > 0 && ui.small_button(crate::trf!("뺀 예시 {hidden_n}장 되살리기", "Restore {hidden_n} removed examples")).clicked() {
                self.lib_meta_mut(&key).hidden.clear();
                self.lib_dirty = true;
                self.dirty = true;
            }
            if ex.is_empty() {
                ui.label(egui::RichText::new(if e.kind.is_preset() {
                    tr("이 프리셋으로 만든 그림(g_이름_번호)이 imgs에 없습니다.", "There are no pictures made with this preset (g_name_number) in imgs.")
                } else {
                    tr("프롬프트에 이 태그들이 (거의) 다 들어간 그림이 없습니다.\n목록에서 그림을 고른 뒤 '고른 그림 고정'으로 직접 넣을 수 있습니다.", "No picture has (nearly) all these tags in its prompt.\nSelect pictures in the list and add them by hand with 'Pin selected pictures'.")
                }).color(DIM));
            }
            let th = 110.0;
            let tw = (th * self.aspect).round();
            let mut act: Option<(String, u8)> = None; // 0 open, 1 cover, 2 pin, 3 unpin, 4 hide
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for id in ex.iter().take(60) {
                    let (rect, resp) = ui.allocate_exact_size(vec2(tw, th), Sense::click());
                    let p = ui.painter().clone();
                    let shown_r = match self.thumb(id) {
                        Some(t) => paint_fit(&p, rect, &t),
                        None => {
                            p.rect_filled(rect.shrink(8.0), 3.0, Color32::from_rgb(36, 36, 40));
                            rect
                        }
                    };
                    if self.sel.contains(id) {
                        p.rect_stroke(rect, 2.0, Stroke::new(1.5, SEL_STROKE), egui::StrokeKind::Inside);
                    }
                    if let Some(im) = self.img(id) {
                        if im.score > 0 {
                            let c = pos2(shown_r.left() + 10.0, shown_r.top() + 10.0);
                            p.circle_filled(c, 8.0, score_color(im.score));
                            p.text(c, egui::Align2::CENTER_CENTER, im.score.to_string(), egui::FontId::proportional(11.0), Color32::WHITE);
                        }
                    }
                    let pinned = pins.contains(id);
                    if pinned || cover.as_deref() == Some(id.as_str()) {
                        p.text(pos2(shown_r.right() - 4.0, shown_r.top() + 2.0), egui::Align2::RIGHT_TOP, if cover.as_deref() == Some(id.as_str()) { "★" } else { "📌" }, egui::FontId::proportional(12.0), Color32::WHITE);
                    }
                    if resp.double_clicked() {
                        act = Some((id.clone(), 0));
                    } else if resp.clicked() {
                        self.sel.clear();
                        self.sel.insert(id.clone());
                    }
                    resp.context_menu(|ui| {
                        if ui.button(tr("크게 보기", "View larger")).clicked() {
                            act = Some((id.clone(), 0));
                            ui.close();
                        }
                        if ui.button(tr("표지로 (카드 대표 그림)", "Use as cover (card picture)")).clicked() {
                            act = Some((id.clone(), 1));
                            ui.close();
                        }
                        if pinned {
                            if ui.button(tr("고정 풀기", "Unpin")).clicked() {
                                act = Some((id.clone(), 3));
                                ui.close();
                            }
                        } else if ui.button(tr("📌 고정 (앞쪽에 늘 보이게)", "📌 Pin (always show first)")).clicked() {
                            act = Some((id.clone(), 2));
                            ui.close();
                        }
                        if ui.button(tr("예시에서 빼기", "Remove from examples")).clicked() {
                            act = Some((id.clone(), 4));
                            ui.close();
                        }
                    });
                }
            });
            if ex.len() > 60 {
                ui.label(egui::RichText::new(crate::trf!("… 외 {}장 ('예시 모두 보기')", "… and {} more ('Show all examples')", ex.len() - 60)).color(DIM).small());
            }
            if let Some((id, a)) = act {
                match a {
                    0 => {
                        let q = format!("lib:{key}");
                        self.open_in_viewer(&id, Some(q), View::Library);
                    }
                    1 => self.lib_pin(&key, std::slice::from_ref(&id), true),
                    2 => self.lib_pin(&key, std::slice::from_ref(&id), false),
                    3 => self.lib_unpin(&key, &id, false),
                    _ => self.lib_unpin(&key, &id, true),
                }
            }
        });
        let _: Option<Vec2> = None;
    }
}
