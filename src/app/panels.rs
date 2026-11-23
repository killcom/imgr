//! Top bar, left filter panel, right detail panel, status bar.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Color32, Key, Modifiers, Sense, Stroke, vec2};

use super::*;
use crate::i18n::tr;

impl App {
    pub(super) fn ui_top(&mut self, ui: &mut egui::Ui) {
        // a narrow window: the search box takes the first row, the buttons wrap to the next
        ui.horizontal_wrapped(|ui| {
            // ⚙ 설정 (top left)
            let r = ui.button("⚙").on_hover_text(tr("설정", "Settings"));
            self.mark("settings_btn", r.rect);
            if r.clicked() {
                self.modal = Some(Modal::Settings { per_folder: self.settings.per_folder.to_string(), ctl: self.settings.ctl });
            }
            // the room right of the ⚙ (a wrapped row reports the whole row as available)
            let room = ui.max_rect().right() - r.rect.right() - ui.spacing().item_spacing.x;
            let one_row = room - self.top_rest_w - 12.0 >= 220.0;
            let id = egui::Id::new("search");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.query_text)
                    .id(id)
                    .hint_text(tr("검색  예) #pixiv score>=4 bikini   (Ctrl+F)", "Search  e.g. #pixiv score>=4 bikini   (Ctrl+F)"))
                    // room for the buttons on the right, as wide as they were last frame
                    .desired_width(if one_row { room - self.top_rest_w - 12.0 } else { (room - 8.0).max(160.0) }),
            );
            self.mark("search", resp.rect);
            let search_right = resp.rect.right();
            let search_mid = resp.rect.center().y;
            if self.focus_search {
                self.focus_search = false;
                resp.request_focus();
            }
            if resp.changed() {
                if matches!(self.view, View::Viewer | View::Compare) {
                    self.view = View::Grid;
                }
                self.refilter(false);
                self.settings.query = self.query_text.clone();
            }
            if resp.lost_focus() {
                let q = self.query_text.clone();
                self.settings.push_search(&q);
            }
            if resp.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                resp.surrender_focus();
            }
            // recent searches
            let mut pick: Option<String> = None;
            let r = ui.add_enabled(!self.settings.search_history.is_empty(), egui::Button::new("▾")).on_hover_text(tr("최근 검색", "Recent searches"));
            egui::Popup::menu(&r).show(|ui| {
                ui.set_min_width(320.0);
                for q in &self.settings.search_history {
                    if ui.button(q).clicked() {
                        pick = Some(q.clone());
                    }
                }
            });
            if let Some(q) = pick {
                self.set_query(q);
            }
            let r = ui.add_enabled(!self.query_text.is_empty(), egui::Button::new("✕")).on_hover_text(tr("검색 지우기", "Clear search"));
            if r.clicked() {
                self.set_query(String::new());
            }
            let r = ui.button("?").on_hover_text(tr("검색 문법·단축키 (F1)", "Search syntax & shortcuts (F1)"));
            self.mark("help", r.rect);
            if r.clicked() {
                self.modal = Some(Modal::Help);
            }
            ui.separator();
            let before = self.sort;
            let q_sort = Query::parse(&self.query_text).sort;
            let shown_label = match q_sort {
                Some(qs) => trf!("{} (검색)", "{} (search)", qs.label()),
                None => self.sort.label().to_string(),
            };
            let r = egui::ComboBox::from_id_salt("sort").selected_text(shown_label).width(118.0).show_ui(ui, |ui| {
                for s in Sort::ALL {
                    ui.selectable_value(&mut self.sort, s, s.label());
                }
            });
            self.mark("sort", r.response.rect);
            if self.sort != before {
                self.settings.sort = self.sort.key().into();
                if self.sort == Sort::Random {
                    self.rand_seed = fastrand::u64(..);
                }
                self.refilter(true);
                self.scroll_to = Some(0);
            }
            ui.separator();
            let r = ui.button(tr("＋ 가져오기", "＋ Import")).on_hover_text(tr("다른 폴더 그림을 imgs로 넣기 (창에 끌어다 놓기·Ctrl+V도 됨)", "Add pictures from another folder to imgs (drag onto the window or Ctrl+V also work)"));
            self.mark("import", r.rect);
            if r.clicked() {
                self.modal = Some(Modal::Import { dir: self.settings.import_dir.clone(), mv: self.settings.import_move, watch_new: String::new() });
            }
            let r = ui.button("⟳").on_hover_text(tr("imgs 다시 읽기 (F5)", "Rescan imgs (F5)"));
            self.mark("rescan", r.rect);
            if r.clicked() {
                self.start_scan(false);
            }
            // measured only while everything is on one row
            let w = (r.rect.right() - search_right).max(200.0);
            let same_row = (r.rect.center().y - search_mid).abs() < 4.0;
            if one_row && same_row && (w - self.top_rest_w).abs() > 1.0 {
                self.top_rest_w = w;
                ui.ctx().request_repaint();
            } else if one_row && !same_row {
                // the guess was too small and a button wrapped: widen it until they fit
                self.top_rest_w += 40.0;
                ui.ctx().request_repaint();
            }
        });
    }

    pub(super) fn ui_left(&mut self, ui: &mut egui::Ui) {
        let mut new_q: Option<String> = None;
        // the 검색 뷰 filters search all pictures: in the album view they start without the album
        let q = if self.view == View::Albums { query::without_album(&self.query_text) } else { self.query_text.clone() };
        let mut action: Option<LeftAction> = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let live = self.db.images.len().saturating_sub(self.missing_n + self.trash_n);
            ui.label(egui::RichText::new(trf!("보이는 것 {} / 전체 {}", "Shown {} / total {}", self.shown.len(), live)).color(DIM));
            ui.add_space(2.0);
            // 앨범 뷰: albums as folders (Explorer-like)
            egui::CollapsingHeader::new(egui::RichText::new(tr("🗂 앨범 뷰", "🗂 Album view")).color(ACCENT).strong().size(15.5 + UI_FONT_PLUS)).id_salt("sec_albumview").default_open(true).show(ui, |ui| {
                self.ui_album_tree(ui, &q, &mut new_q, &mut action, true);
            });
            ui.add_space(6.0);
            // 검색 뷰: all pictures, filtered
            egui::CollapsingHeader::new(egui::RichText::new(tr("▦ 검색 뷰", "▦ Search view")).color(ACCENT).strong().size(15.5 + UI_FONT_PLUS)).id_salt("sec_searchview").default_open(true).show(ui, |ui| {
            let r = ui.selectable_label(self.view == View::Grid && q.trim().is_empty(), tr("전체 보기", "Show all")).on_hover_text(tr("모든 그림 (검색 지우기)", "All pictures (clears the search)"));
            self.mark("all_view", r.rect);
            if r.clicked() {
                new_q = Some(String::new());
            }
            egui::CollapsingHeader::new(egui::RichText::new(tr("저장한 검색", "Saved searches")).color(ACCENT).strong()).id_salt("sec_filters").default_open(true).show(ui, |ui| {
                let n = self.db.saved.len();
                for k in 0..n {
                    let f = self.db.saved[k].clone();
                    let r = ui
                        .selectable_label(q.trim() == f.query.trim(), format!("▸ {}", f.name))
                        .on_hover_text(trf!("{}\n(오른쪽 클릭: 메뉴)", "{}\n(right-click: menu)", f.query));
                    if r.clicked() {
                        new_q = Some(f.query.clone());
                    }
                    r.context_menu(|ui| {
                        if ui.button(tr("이름 바꾸기", "Rename")).clicked() {
                            action = Some(LeftAction::RenameFilter(k));
                            ui.close();
                        }
                        if ui.add_enabled(!q.trim().is_empty() && q.trim() != f.query.trim(), egui::Button::new(tr("지금 검색으로 바꾸기", "Replace with current search"))).clicked() {
                            action = Some(LeftAction::UpdateFilter(k));
                            ui.close();
                        }
                        if ui.add_enabled(k > 0, egui::Button::new(tr("위로", "Move up"))).clicked() {
                            action = Some(LeftAction::MoveFilter(k, -1));
                            ui.close();
                        }
                        if ui.add_enabled(k + 1 < n, egui::Button::new(tr("아래로", "Move down"))).clicked() {
                            action = Some(LeftAction::MoveFilter(k, 1));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(tr("지우기", "Delete")).clicked() {
                            action = Some(LeftAction::DeleteFilter(k));
                            ui.close();
                        }
                    });
                }
                let r = ui.add_enabled(!q.trim().is_empty(), egui::Button::new(tr("＋ 지금 검색을 필터로 저장", "＋ Save current search as filter")));
                self.mark("save_filter", r.rect);
                if r.clicked() {
                    self.modal = Some(Modal::SaveFilter { name: String::new() });
                }
            });

            egui::CollapsingHeader::new(egui::RichText::new(tr("점수", "Score")).color(ACCENT).strong()).id_salt("sec_score").default_open(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    // N/A and 1~6 share the panel width (one row, never wrapped)
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.spacing_mut().button_padding = vec2(2.0, 1.0);
                    let bw = ((ui.available_width() - 6.0 * 3.0) / 7.0).clamp(18.0, 30.0);
                    // N/A = no score yet (a score button and N/A rule each other out)
                    let na = query::has_word(&q, "is:unscored");
                    let b = egui::Button::new(egui::RichText::new("N/A").color(Color32::WHITE).strong().size(10.5))
                        .fill(if na { Color32::from_gray(110) } else { Color32::from_gray(60) })
                        .stroke(if na { Stroke::new(1.5, Color32::WHITE) } else { Stroke::NONE })
                        .min_size(vec2(bw, 22.0));
                    let r = ui.add_sized(vec2(bw, 22.0), b).on_hover_text(trf!("점수 없는 그림 {}장
클릭: 이것만 보기", "{} pictures without a score
Click: show only these", self.score_counts[0]));
                    self.mark("score_na", r.rect);
                    if r.clicked() {
                        let base = query::set_prefixed(&q, "score", None);
                        new_q = Some(if na { query::toggle_word(&base, "is:unscored") } else if query::has_word(&base, "is:unscored") { base } else { query::toggle_word(&base, "is:unscored") });
                    }
                    for n in 1..=6u8 {
                        let w_eq = format!("score:{n}");
                        let w_ge = format!("score>={n}");
                        let on = query::has_word(&q, &w_eq) || query::has_word(&q, &w_ge);
                        let cnt = self.score_counts[n as usize];
                        let txt = egui::RichText::new(format!("{n}")).color(Color32::WHITE).strong();
                        let b = egui::Button::new(txt)
                            .fill(if on { score_color(n) } else { score_color(n).gamma_multiply(0.45) })
                            .stroke(if query::has_word(&q, &w_ge) { Stroke::new(1.5, Color32::WHITE) } else { Stroke::NONE })
                            .min_size(vec2(bw, 22.0));
                        let r = ui.add_sized(vec2(bw, 22.0), b).on_hover_text(trf!("{n}점 {cnt}장\n클릭: {n}점만  /  오른쪽 클릭: {n}점 이상", "Score {n}: {cnt} pictures\nClick: only {n}  /  right-click: {n} or more"));
                        let q0 = if query::has_word(&q, "is:unscored") { query::toggle_word(&q, "is:unscored") } else { q.clone() };
                        if r.clicked() {
                            new_q = Some(query::set_prefixed(&q0, "score", if query::has_word(&q, &w_eq) { None } else { Some(&w_eq) }));
                        }
                        if r.secondary_clicked() {
                            new_q = Some(query::set_prefixed(&q0, "score", if query::has_word(&q, &w_ge) { None } else { Some(&w_ge) }));
                        }
                    }
                });
                // score distribution: one thin bar per score
                let total: usize = self.score_counts[1..].iter().sum::<usize>().max(1);
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(200.0), 6.0), Sense::hover());
                let mut x = rect.left();
                for n in 1..=6usize {
                    let w = rect.width() * self.score_counts[n] as f32 / total as f32;
                    ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(w, rect.height())), 0.0, score_color(n as u8));
                    x += w;
                }
            });

            egui::CollapsingHeader::new(egui::RichText::new(trf!("해시태그 {}", "Hashtags {}", self.tag_counts.len())).color(ACCENT).strong())
                .id_salt("sec_tags")
                .default_open(true)
                .show(ui, |ui| {
                    if self.tag_counts.is_empty() {
                        ui.label(egui::RichText::new(tr("아직 없음. 그림을 고르고 T 를 눌러 붙이세요", "None yet. Pick a picture and press T to add one")).color(DIM).small());
                    }
                    ui.horizontal_wrapped(|ui| {
                        if !self.tag_counts.is_empty() && ui.selectable_label(query::has_word(&q, "is:untagged"), tr("해시태그 없음", "no hashtags")).on_hover_text(tr("해시태그가 하나도 없는 그림", "Pictures without any hashtag")).clicked() {
                            new_q = Some(query::toggle_word(&q, "is:untagged"));
                        }
                        for (t, n) in &self.tag_counts {
                            let w = format!("#{t}");
                            let neg = format!("-#{t}");
                            let on = query::has_word(&q, &w);
                            let off = query::has_word(&q, &neg);
                            let label = if off { format!("✕#{t} {n}") } else { format!("#{t} {n}") };
                            let r = ui.selectable_label(on || off, label).on_hover_text(tr("클릭: 이 태그만 보기 / 오른쪽 클릭: 메뉴", "Click: show only this tag / right-click: menu"));
                            if r.clicked() {
                                new_q = Some(query::toggle_word(&q, &w));
                            }
                            r.context_menu(|ui| {
                                if ui.button(tr("이 태그 빼고 보기", "Show without this tag")).clicked() {
                                    new_q = Some(query::toggle_word(&q, &neg));
                                    ui.close();
                                }
                                if ui.button(tr("고른 그림에 붙이기/떼기", "Add/remove on picked pictures")).clicked() {
                                    action = Some(LeftAction::ToggleTag(t.clone()));
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button(tr("이름 바꾸기 · 다른 태그에 합치기…", "Rename · merge into another tag…")).clicked() {
                                    action = Some(LeftAction::RenameTag(t.clone()));
                                    ui.close();
                                }
                                if ui.button(trf!("모든 그림에서 떼기 ({n}장)…", "Remove from all pictures ({n})…")).clicked() {
                                    action = Some(LeftAction::DeleteTag(t.clone(), *n));
                                    ui.close();
                                }
                            });
                        }
                    });
                });

            egui::CollapsingHeader::new(egui::RichText::new(tr("앨범", "Album")).color(ACCENT).strong()).id_salt("sec_album").default_open(true).show(ui, |ui| {
                self.ui_album_tree(ui, &q, &mut new_q, &mut action, false);
            });
            egui::CollapsingHeader::new(egui::RichText::new(tr("체크포인트", "Checkpoint")).color(ACCENT).strong()).id_salt("sec_ckpt").default_open(true).show(ui, |ui| {
                list_counts(ui, &self.ckpt_counts, "ckpt", &q, &mut new_q, &mut action);
            });
            egui::CollapsingHeader::new(egui::RichText::new("LoRA").color(ACCENT).strong()).id_salt("sec_lora").default_open(false).show(ui, |ui| {
                list_counts(ui, &self.lora_counts, "lora", &q, &mut new_q, &mut action);
            });
            // 태그 여부: ComfyUI pictures (workflow info kept) and plain ones; both on = all
            egui::CollapsingHeader::new(egui::RichText::new(tr("태그 여부", "Workflow info")).color(ACCENT).strong()).id_salt("sec_kind").default_open(true).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let (wf, plain) = (self.settings.kind_wf, self.settings.kind_plain);
                    let r = ui.selectable_label(wf, trf!("워크플로 {}", "workflow {}", self.kind_counts.0)).on_hover_text(tr("ComfyUI 정보(프롬프트·체크포인트·시드)가 든 그림
둘 다 켜면 모두, 하나만 켜면 그것만 (앨범 뷰에는 적용 안 됨)", "Pictures with ComfyUI info (prompt, checkpoint, seed)
Both on = all, only one on = just those (not applied in Album view)"));
                    self.mark("kind_wf", r.rect);
                    if r.clicked() && !(wf && !plain) {
                        action = Some(LeftAction::Kind(!wf, plain));
                    }
                    let r = ui.selectable_label(plain, trf!("태그없음 {}", "no workflow {}", self.kind_counts.1)).on_hover_text(tr("ComfyUI 정보가 없는 그림 (보통 jpg 등)
둘 다 켜면 모두, 하나만 켜면 그것만 (앨범 뷰에는 적용 안 됨)", "Pictures without ComfyUI info (usually jpg etc.)
Both on = all, only one on = just those (not applied in Album view)"));
                    self.mark("kind_plain", r.rect);
                    if r.clicked() && !(plain && !wf) {
                        action = Some(LeftAction::Kind(wf, !plain));
                    }
                });
            });
            egui::CollapsingHeader::new(egui::RichText::new(tr("폴더", "Folder")).color(ACCENT).strong()).id_salt("sec_folder").default_open(false).show(ui, |ui| {
                list_counts(ui, &self.folder_counts, "folder", &q, &mut new_q, &mut action);
            });
            });
        });
        if let Some(a) = action {
            match a {
                LeftAction::RenameFilter(k) => {
                    let name = self.db.saved[k].name.clone();
                    self.modal = Some(Modal::RenameFilter { idx: k, name });
                }
                LeftAction::UpdateFilter(k) => {
                    self.db.saved[k].query = self.query_text.trim().to_string();
                    self.dirty = true;
                    self.notice(trf!("필터 '{}'를 지금 검색으로 바꿈", "Filter '{}' replaced with the current search", self.db.saved[k].name), false);
                }
                LeftAction::MoveFilter(k, d) => {
                    let j = (k as isize + d) as usize;
                    self.db.saved.swap(k, j);
                    self.dirty = true;
                }
                LeftAction::DeleteFilter(k) => {
                    let f = self.db.saved.remove(k);
                    self.dirty = true;
                    self.notice(trf!("필터 '{}' 지움 ({})", "Filter '{}' deleted ({})", f.name, f.query), false);
                }
                LeftAction::ToggleTag(t) => self.toggle_tag(&t),
                LeftAction::Kind(wf, plain) => {
                    self.settings.kind_wf = wf;
                    self.settings.kind_plain = plain;
                    if self.view == View::Albums {
                        self.view = View::Grid;
                        self.set_query(query::without_album(&self.query_text));
                    } else {
                        self.refilter(false);
                    }
                }
                LeftAction::RenameTag(t) => self.modal = Some(Modal::TagRename { new: t.clone(), old: t }),
                LeftAction::DeleteTag(t, n) => self.modal = Some(Modal::TagDelete { tag: t, n }),
                LeftAction::NewAlbum(parent) => self.new_album_under(parent),
                LeftAction::RenameAlbum(id) => {
                    let name = self.album_label(Some(id));
                    self.modal = Some(Modal::AlbumRename { id, new: name });
                }
                LeftAction::DeleteAlbum(id) => self.modal = Some(Modal::AlbumDelete { ids: vec![id] }),
                LeftAction::MoveAlbum(id, parent) => {
                    if self.db.move_album(id, parent) {
                        self.dirty = true;
                        if parent != 0 {
                            self.album_folded.remove(&parent);
                        }
                    }
                }
                LeftAction::OpenAlbum(a) => self.open_album(a),
                LeftAction::AddToAlbum(to) => {
                    let ids = self.selected_ids();
                    self.add_to_album(&ids, to);
                }
            }
        }
        // anything picked in the 검색 뷰 part searches all pictures in the list
        if let Some(nq) = new_q {
            if self.view == View::Albums {
                self.view = View::Grid;
            }
            self.set_query(nq);
        }
    }

    /// Side panel album tree. `nav` (앨범 뷰): 루트, 무소속 and the albums; a click
    /// opens it in the album view. Otherwise (검색 뷰 › 앨범): a click adds or
    /// removes that album as a filter of the search, together with the other
    /// filters. In both, rows are drop spots for dragged pictures.
    fn ui_album_tree(&mut self, ui: &mut egui::Ui, q: &str, new_q: &mut Option<String>, action: &mut Option<LeftAction>, nav: bool) {
        let in_view = self.view == View::Albums;
        let cur = self.cur_album();
        // rows sit right under each other, like Explorer's folder tree
        ui.spacing_mut().item_spacing.y = 1.0;
        let base = if nav { 14.0 } else { 0.0 };
        // 검색 뷰: the albums picked as a filter (album:3|album:none …), like checkpoints
        let group = query::group_values_cased(q, "album");
        // an album row picks the album with its sub albums (album:trip+); album:trip (only that album) counts as picked too
        let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase();
        let is_on = |v: &str| group.iter().any(|x| same(x, v) || x.strip_suffix('+').is_none() && same(&format!("{x}+"), v));
        let pick = |v: &str, ctrl: bool| -> String {
            let on = is_on(v);
            let plain = v.trim_end_matches('+');
            let mut g = group.clone();
            if ctrl {
                if on {
                    g.retain(|x| !same(x, v) && !same(x, plain));
                } else {
                    g.push(v.to_string());
                }
            } else if on && g.len() == 1 {
                g.clear();
            } else {
                g = vec![v.to_string()];
            }
            query::set_group(q, "album", &g)
        };
        if nav {
            let (r, _) = tree_row(ui, 0.0, "/".to_string(), in_view && cur.is_none(), None, "nav_root");
            let r = r.on_hover_text(tr("앨범 뷰의 맨 위 (맨 위 앨범들, 그 뒤에 어느 앨범에도 없는 그림)\n앨범을 여기 끌어다 놓으면 맨 위로 · 오른쪽 클릭: 새 앨범", "Top of Album view (top-level albums, then pictures in no album)\nDrop an album here to move it to the top · right-click: new album"));
            self.mark("nav_root", r.rect);
            self.album_drop.push((Some(0), r.rect.intersect(ui.clip_rect())));
            if r.clicked() {
                *action = Some(LeftAction::OpenAlbum(None));
            }
            r.context_menu(|ui| {
                if ui.button(tr("새 앨범…", "New album…")).clicked() {
                    *action = Some(LeftAction::NewAlbum(0));
                    ui.close();
                }
            });
        }
        // 검색 뷰 only: 무소속 (in the 앨범 뷰 those pictures sit at the top, after the albums)
        if !nav {
            let (r, _) = tree_row(ui, base, trf!("무소속 {}", "No album {}", self.unfiled_n), is_on("none"), None, "album_none");
            let r = r.on_hover_text(tr("어느 앨범에도 없는 그림\n클릭: 이것만 보기 / Ctrl+클릭: 여러 개 같이 보기\n그림을 여기 끌어다 놓으면 모든 앨범에서 빠짐", "Pictures in no album\nClick: show only these / Ctrl+click: show several together\nDrop pictures here to remove them from all albums"));
            self.mark("album_none", r.rect);
            self.album_drop.push((None, r.rect.intersect(ui.clip_rect())));
            if r.clicked() {
                *new_q = Some(pick("none", ui.input(|i| i.modifiers.ctrl)));
            }
        }
        for (id, name, depth, has_kids) in self.album_rows_open() {
            // 앨범 뷰: pictures right in it / 검색 뷰: with its sub albums (what a click there shows)
            let n = if nav { self.album_counts.get(&id) } else { self.album_tree_counts.get(&id) }.copied().unwrap_or(0);
            let v = format!("{}+", self.album_ref(id));
            let on = if nav { in_view && cur == Some(Some(id)) } else { is_on(&v) };
            let fold = has_kids.then(|| self.album_folded.contains(&id));
            let key = if nav { format!("nav_album_{id}") } else { format!("album_{id}") };
            let (r, fold_clicked) = tree_row(ui, base + depth as f32 * 14.0, format!("{name} {n}"), on, fold, &key);
            if fold_clicked {
                if !self.album_folded.remove(&id) {
                    self.album_folded.insert(id);
                }
            }
            let r = r.on_hover_text(if nav {
                tr("클릭: 앨범 뷰에서 열기 / 그림을 끌어다 놓으면 넣기 / 오른쪽 클릭: 메뉴", "Click: open in Album view / drop pictures to add them / right-click: menu")
            } else {
                tr("클릭: 이 앨범만 보기 (하위 앨범 포함) / Ctrl+클릭: 여러 앨범 같이 보기 (다른 조건과 같이) / 그림을 끌어다 놓으면 넣기", "Click: show only this album (with sub albums) / Ctrl+click: show several albums together (with other conditions) / drop pictures to add them")
            });
            self.mark(&key, r.rect);
            self.album_drop.push((Some(id), r.rect.intersect(ui.clip_rect())));
            if r.clicked() {
                if nav {
                    *action = Some(LeftAction::OpenAlbum(Some(Some(id))));
                } else {
                    *new_q = Some(pick(&v, ui.input(|i| i.modifiers.ctrl)));
                }
            }
            r.context_menu(|ui| {
                if ui.button(tr("고른 그림을 이 앨범에 넣기", "Add picked pictures to this album")).clicked() {
                    *action = Some(LeftAction::AddToAlbum(Some(id)));
                    ui.close();
                }
                if ui.button(tr("하위 앨범 만들기", "Create sub album")).clicked() {
                    *action = Some(LeftAction::NewAlbum(id));
                    ui.close();
                }
                if ui.button(tr("이름 바꾸기", "Rename")).clicked() {
                    *action = Some(LeftAction::RenameAlbum(id));
                    ui.close();
                }
                ui.menu_button(tr("옮기기", "Move"), |ui| {
                    let sub = self.db.album_subtree(id);
                    if ui.button(tr("맨 위로", "To top")).clicked() {
                        *action = Some(LeftAction::MoveAlbum(id, 0));
                        ui.close();
                    }
                    for (pid, pname, pdepth) in self.album_rows() {
                        if sub.contains(&pid) {
                            continue;
                        }
                        if ui.button(trf!("{}{} 안으로", "{0}Into {1}", "    ".repeat(pdepth), pname)).clicked() {
                            *action = Some(LeftAction::MoveAlbum(id, pid));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button(tr("앨범 지우기… (그림은 그대로)", "Delete album… (pictures stay)")).clicked() {
                    *action = Some(LeftAction::DeleteAlbum(id));
                    ui.close();
                }
            });
        }
        if nav && self.db.album_children(0).is_empty() {
            ui.label(egui::RichText::new(tr("앨범 뷰의 빈 곳 오른쪽 클릭 또는 Ctrl+N으로 새 앨범", "Right-click an empty spot in Album view or press Ctrl+N for a new album")).color(DIM).small());
        }
        if nav {
            // 휴지통: on the level of "/", after it
            let on = query::has_word(&self.query_text, "is:trash");
            let (r, _) = tree_row(ui, 0.0, trf!("휴지통 {}", "Trash {}", self.trash_n), on, None, "nav_trash");
            let r = r.on_hover_text(tr("imgr 휴지통에 넣은 그림 (파일은 휴지통 비우기 때까지 그대로)
오른쪽 클릭: 휴지통 비우기", "Pictures put in the imgr Trash (files stay until the Trash is emptied)
Right-click: empty Trash"));
            self.mark("nav_trash", r.rect);
            if r.clicked() {
                *new_q = Some(if on { String::new() } else { "is:trash".to_string() });
            }
            let n = self.trash_n;
            r.context_menu(|ui| {
                if ui.add_enabled(n > 0, egui::Button::new(trf!("휴지통 비우기 ({n})…", "Empty Trash ({n})…"))).clicked() {
                    self.modal = Some(Modal::EmptyTrash { n });
                    ui.close();
                }
            });
        }
    }

    pub(super) fn ui_right(&mut self, ui: &mut egui::Ui) {
        let ids = self.picked_pictures();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if ids.is_empty() && !self.album_sel.is_empty() {
                ui.add_space(8.0);
                let mut sel: Vec<u32> = self.album_sel.iter().copied().collect();
                sel.sort_unstable();
                ui.label(egui::RichText::new(trf!("고른 앨범 {}개", "{} albums picked", sel.len())).color(ACCENT).strong());
                for id in sel.iter().take(30) {
                    let n = self.album_counts.get(id).copied().unwrap_or(0);
                    let kids = self.db.album_children(*id).len();
                    ui.label(trf!("📁 {}  —  {n}장{}", "📁 {}  —  {n} pictures{}", self.db.album_path(*id), if kids > 0 { trf!(" · 앨범 {kids}", " · {kids} albums") } else { String::new() }));
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tr("Enter 열기 · F2 이름 · Del 지우기 · 다른 앨범에 끌어 놓기 / Ctrl+X → Ctrl+V 옮기기", "Enter open · F2 rename · Del delete · drag onto another album / Ctrl+X → Ctrl+V to move")).color(DIM).small());
                return;
            }
            if ids.is_empty() {
                ui.add_space(20.0);
                ui.label(egui::RichText::new(tr("그림을 고르면 여기에 정보가 나옵니다", "Pick a picture to see its info here")).color(DIM));
                return;
            }
            let focus_id = if self.view == View::Compare { self.cmp_ids.get(self.cmp_focus).cloned() } else { self.focus_img().map(|i| i.id.clone()) };
            let main_id = focus_id.filter(|f| ids.contains(f)).unwrap_or_else(|| ids[0].clone());
            let Some(im) = self.img(&main_id).cloned() else { return };

            // preview (only in grid / matrix; viewer and compare already show it big)
            if matches!(self.view, View::Grid | View::Matrix | View::Albums) {
                let w = ui.available_width();
                let ar = if im.w > 0 { im.h as f32 / im.w as f32 } else { 1.4 };
                let win_h = ui.ctx().content_rect().height();
                let h = (w * ar).min(win_h * self.settings.preview_frac.clamp(0.1, 0.9));
                let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
                // corner handle: drag to change the panel width and the preview height together
                let hr = egui::Rect::from_min_size(egui::pos2(rect.left() - 2.0, rect.bottom() - 16.0), vec2(18.0, 18.0));
                let hresp = ui.interact(hr, egui::Id::new("preview_corner"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeNeSw).on_hover_text(tr("끌어서 패널 너비와 미리보기 크기를 같이 바꾸기", "Drag to resize the panel and the preview together"));
                self.mark("preview_corner", hr);
                let hot = hresp.hovered() || hresp.dragged();
                let col = if hot { Color32::WHITE } else { Color32::from_gray(150) };
                let p = ui.painter();
                p.rect_filled(hr.shrink(1.0), 3.0, Color32::from_black_alpha(if hot { 200 } else { 140 }));
                for k in [4.0, 8.0, 12.0] {
                    p.line_segment([egui::pos2(hr.left() + 3.0, hr.bottom() - 3.0 - k), egui::pos2(hr.left() + 3.0 + k, hr.bottom() - 3.0)], Stroke::new(1.2, col));
                }
                self.corner_drag = hresp.dragged();
                if hresp.dragged() {
                    let d = hresp.drag_delta();
                    self.settings.right_w = (self.settings.right_w - d.x).clamp(220.0, 900.0);
                    if win_h > 0.0 {
                        self.settings.preview_frac = (self.settings.preview_frac + d.y / win_h).clamp(0.1, 0.9);
                    }
                }
                match self.big_or_thumb(&im.id, PREVIEW_PX) {
                    Some(t) => {
                        paint_fit(ui.painter(), rect, &t);
                    }
                    None => {
                        ui.painter().rect_filled(rect.shrink(20.0), 4.0, Color32::from_rgb(36, 36, 40));
                    }
                }
                if resp.double_clicked() {
                    self.open_viewer(self.view);
                }
            }
            if ids.len() > 1 {
                let total: u64 = ids.iter().filter_map(|id| self.img(id)).map(|i| i.size).sum();
                ui.label(egui::RichText::new(trf!("{}장 선택됨 · {}  (점수·태그는 모두에 적용)", "{} pictures selected · {}  (score and tags apply to all)", ids.len(), human_size(total))).color(ACCENT).strong());
            }
            // the prompt right under the picture (the file name is in 패스 below)
            if !im.prompt.is_empty() {
                egui::CollapsingHeader::new(tr("프롬프트", "Prompt")).id_salt("prompt_hdr").default_open(true).show(ui, |ui| {
                    // read-only, selectable (a text box would steal the keyboard)
                    ui.add(egui::Label::new(egui::RichText::new(&im.prompt).size(14.5 + UI_FONT_PLUS)).selectable(true).wrap());
                    ui.horizontal(|ui| {
                        if ui.small_button(tr("복사", "Copy")).clicked() {
                            ui.ctx().copy_text(im.prompt.clone());
                        }
                    });
                });
            }

            // score
            ui.add_space(2.0);
            let cur: Option<u8> = {
                let set: HashSet<u8> = ids.iter().filter_map(|id| self.img(id)).map(|i| i.score).collect();
                if set.len() == 1 { set.into_iter().next() } else { None }
            };
            let mut score_click = None;
            let srow = ui.horizontal(|ui| {
                for n in 1..=6u8 {
                    let on = cur == Some(n);
                    let b = egui::Button::new(egui::RichText::new(format!("{n}")).size(18.0).color(Color32::WHITE))
                        .fill(if on { score_color(n) } else { score_color(n).gamma_multiply(0.35) })
                        .stroke(if on { Stroke::new(1.5, Color32::WHITE) } else { Stroke::NONE })
                        .min_size(vec2(36.0, 32.0));
                    if ui.add(b).on_hover_text(trf!("{n}점 (키보드 {n})", "Score {n} (key {n})")).clicked() {
                        score_click = Some(if on { 0 } else { n });
                    }
                }
                if ui.add(egui::Button::new("✕").min_size(vec2(28.0, 32.0))).on_hover_text(tr("점수 지우기 (0)", "Clear score (0)")).clicked() {
                    score_click = Some(0);
                }
            });
            self.mark("score_row", srow.response.rect);
            if let Some(n) = score_click {
                self.set_score(n);
            }

            // tags
            ui.add_space(4.0);
            let mut counts: HashMap<String, usize> = HashMap::new();
            for id in &ids {
                if let Some(i) = self.img(id) {
                    for t in &i.tags {
                        *counts.entry(t.clone()).or_default() += 1;
                    }
                }
            }
            let mut tv: Vec<(String, usize)> = counts.into_iter().collect();
            tv.sort();
            let mut remove: Option<String> = None;
            let mut toggle: Option<String> = None;
            let mut add_list: Vec<String> = Vec::new();
            let mut search_tag: Option<String> = None;
            ui.horizontal_wrapped(|ui| {
                for (t, n) in &tv {
                    let partial = ids.len() > 1 && *n < ids.len();
                    let label = if partial { format!("#{t} ({n})") } else { format!("#{t}") };
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let r = ui.add(egui::Button::new(egui::RichText::new(label).color(TEXT)).fill(TAG_BG)).on_hover_text(if partial { tr("일부에만 있음. 클릭: 이 태그로 찾기 / 오른쪽 클릭: 모두에 붙이기", "On some only. Click: search this tag / right-click: add to all") } else { tr("클릭: 이 태그로 찾기", "Click: search this tag") });
                    if r.clicked() {
                        search_tag = Some(t.clone());
                    }
                    if r.secondary_clicked() {
                        add_list.push(t.clone());
                    }
                    let x = ui.add(egui::Button::new(egui::RichText::new("✕").color(DIM)).fill(TAG_BG)).on_hover_text(if partial { tr("모두에서 떼기", "Remove from all") } else { tr("떼기", "Remove") });
                    if x.clicked() {
                        remove = Some(t.clone());
                    }
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.add_space(6.0);
                }
            });
            if let Some(t) = search_tag {
                let q = query::toggle_word(&self.query_text, &format!("#{t}"));
                self.set_query(q);
            }
            // tag input with suggestions (Tab = first suggestion)
            let id = egui::Id::new("tag_input");
            let typed = clean_tag(&self.tag_input.rsplit([',', ' ']).next().unwrap_or("").to_string());
            let sugg: Vec<String> = if typed.is_empty() {
                Vec::new()
            } else {
                let mut v: Vec<String> = self.tag_counts.iter().map(|(t, _)| t.clone()).filter(|t| t.starts_with(&typed) && *t != typed).take(6).collect();
                for (t, _) in &self.tag_counts {
                    if v.len() >= 8 {
                        break;
                    }
                    if !t.starts_with(&typed) && t.contains(&typed) && !v.contains(t) {
                        v.push(t.clone());
                    }
                }
                v
            };
            let focused_before = ui.ctx().memory(|m| m.has_focus(id));
            if focused_before && !sugg.is_empty() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) {
                let base = self.tag_input.rsplit_once([',', ' ']).map(|(a, _)| format!("{a} ")).unwrap_or_default();
                self.tag_input = format!("{base}{}", sugg[0]);
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                    let n = self.tag_input.chars().count();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(n))));
                    st.store(ui.ctx(), id);
                }
            }
            let r = ui.add(egui::TextEdit::singleline(&mut self.tag_input).id(id).hint_text(tr("해시태그 붙이기 (T · Enter · Tab 자동완성)", "Add hashtag (T · Enter · Tab autocompletes)")).lock_focus(true).desired_width(f32::INFINITY));
            self.mark("tag_input", r.rect);
            if self.focus_tag_input {
                self.focus_tag_input = false;
                r.request_focus();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let txt = std::mem::take(&mut self.tag_input);
                for t in txt.split([',', ' ']) {
                    if !t.trim().is_empty() {
                        add_list.push(t.to_string());
                    }
                }
                // stay in the box for the next tag; Enter on an empty box = done (keys work again)
                if !add_list.is_empty() {
                    r.request_focus();
                }
            }
            if r.has_focus() {
                ui.label(egui::RichText::new(tr("빈 칸에서 Enter 또는 Esc = 입력 끝 (그동안 단축키는 쉼)", "Enter or Esc in an empty box = done typing (shortcuts are paused meanwhile)")).color(DIM).small());
            }
            if r.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                r.surrender_focus();
            }
            if !sugg.is_empty() && r.has_focus() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tr("제안", "Suggestions")).color(DIM).small());
                    for s in &sugg {
                        if ui.small_button(format!("#{s}")).clicked() {
                            add_list.push(s.clone());
                            self.tag_input.clear();
                        }
                    }
                });
            }
            let mut known: Vec<String> = self.settings.recent_tags.iter().filter(|t| !tv.iter().any(|(x, n)| x == *t && *n == ids.len())).cloned().collect();
            for (t, _) in &self.tag_counts {
                if known.len() >= 14 {
                    break;
                }
                if !known.contains(t) && !tv.iter().any(|(x, n)| x == t && *n == ids.len()) {
                    known.push(t.clone());
                }
            }
            if !known.is_empty() {
                let rr = ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tr("빠른 태그", "Quick tags")).color(DIM).small());
                    for t in &known {
                        if ui.small_button(format!("+#{t}")).clicked() {
                            toggle = Some(t.clone());
                        }
                    }
                });
                self.mark("quick_tags", rr.response.rect);
            }
            if let Some(t) = remove {
                self.remove_tag(&t);
            }
            if let Some(t) = toggle {
                self.toggle_tag(&t);
            }
            for t in add_list {
                self.add_tag(&t);
            }

            // info
            ui.add_space(6.0);
            ui.separator();
            egui::Grid::new("info").num_columns(2).spacing(vec2(8.0, 3.0)).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(egui::RichText::new(k).color(DIM));
                    ui.add(egui::Label::new(v).wrap());
                    ui.end_row();
                };
                row(ui, tr("패스", "Path"), im.path.clone());
                row(ui, tr("크기", "Size"), format!("{}×{}  {}", im.w, im.h, human_size(im.size)));
                row(ui, tr("시각", "Time"), trf!("파일 {}  ·  넣음 {}", "file {}  ·  added {}", fmt_time(im.mtime as i64), fmt_time(im.added)));
                if !im.preset.is_empty() {
                    row(ui, tr("프리셋", "Preset"), im.preset.clone());
                }
                if !im.ckpt.is_empty() {
                    row(ui, tr("체크포인트", "Checkpoint"), im.ckpt.clone());
                }
                if !im.loras.is_empty() {
                    row(ui, "LoRA", im.loras.join(", "));
                }
                if let Some(s) = im.seed {
                    row(ui, tr("시드", "Seed"), s.to_string());
                }
            });
            let mut nq = None;
            ui.horizontal_wrapped(|ui| {
                if let Some(s) = im.seed {
                    if ui.button(tr("같은 시드", "Same seed")).on_hover_text(tr("같은 큐·시드로 뽑은 다른 프리셋/체크포인트 그림 (비교용)", "Pictures from the same queue and seed with another preset/checkpoint (for comparing)")).clicked() {
                        nq = Some(format!("seed:{s} sort:seed"));
                    }
                }
                if !im.preset.is_empty() && ui.button(tr("같은 프리셋", "Same preset")).clicked() {
                    nq = Some(format!("preset:{}", im.preset));
                }
            });
            // albums of this picture: click = open that album
            let mut open_al: Option<u32> = None;
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tr("앨범", "Album")).color(DIM));
                if im.albums.is_empty() {
                    ui.label(egui::RichText::new(tr("없음", "none")).color(DIM));
                }
                for a in &im.albums {
                    if self.db.album(*a).is_some() && ui.small_button(self.db.album_path(*a)).on_hover_text(tr("이 앨범 열기", "Open this album")).clicked() {
                        open_al = Some(*a);
                    }
                }
            });
            if let Some(a) = open_al {
                self.open_album(Some(Some(a)));
            }
            ui.horizontal_wrapped(|ui| {
                let path = self.img_path(&im);
                if ui.button(tr("파일 열기", "Open file")).clicked() {
                    winshell::open_file(&path);
                }
                let r = ui.button(tr("탐색기에서 보기", "Show in Explorer")).on_hover_text(tr("imgs 안에서 이 그림이 있는 폴더를 열고 그림을 골라 둠", "Opens this picture's folder inside imgs with the picture selected"));
                self.mark("reveal_btn", r.rect);
                if r.clicked() {
                    // this picture's own folder first (the other picked ones in it are selected too)
                    let mut paths = vec![path.clone()];
                    paths.extend(self.paths_of(&ids).into_iter().filter(|p| *p != path));
                    winshell::reveal(&paths);
                }
                if ui.button(tr("이름 (F2)", "Rename (F2)")).clicked() {
                    self.open_rename();
                }
                if ui.button(tr("내보내기", "Export")).on_hover_text(tr("업로드용 복사 (Ctrl+E)", "Copy for uploading (Ctrl+E)")).clicked() {
                    self.open_export();
                }
                let trash_lbl = if im.trash { tr("되살리기", "Restore") } else { tr("휴지통 (Del)", "Trash (Del)") };
                if ui.button(trash_lbl).clicked() {
                    self.set_trash(!im.trash);
                }
            });
            if let Some(q) = nq {
                self.set_query(q);
            }
        });
    }

    pub(super) fn ui_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some((d, t)) = self.scan_progress {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(if t > 0 { trf!("imgs 읽는 중 {d}/{t}", "Reading imgs {d}/{t}") } else { tr("imgs 읽는 중…", "Reading imgs…").into() });
                ui.separator();
            }
            if let Some((d, t)) = self.import_progress {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(trf!("가져오는 중 {d}/{t}", "Importing {d}/{t}"));
                ui.separator();
            }
            if let Some((d, t)) = self.export_progress {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(trf!("내보내는 중 {d}/{t}", "Exporting {d}/{t}"));
                ui.separator();
            }
            if let Some(n) = self.notices.back() {
                let r = ui.label(egui::RichText::new(&n.text).color(if n.error { ERR } else { OK }));
                if self.notices.len() > 1 {
                    let all: Vec<String> = self.notices.iter().rev().map(|n| n.text.clone()).collect();
                    r.on_hover_text(all.join("\n"));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.add(egui::Slider::new(&mut self.settings.thumb_size, 70.0..=520.0).show_value(false)).on_hover_text(tr("썸네일 크기 (Ctrl+휠)", "Thumbnail size (Ctrl+wheel)"));
                self.mark("thumb_slider", r.rect);
                ui.checkbox(&mut self.settings.show_names, tr("파일 이름", "File names")).on_hover_text(tr("썸네일 아래 파일 이름 보이기", "Show file names under thumbnails"));
                if ui.selectable_label(self.settings.show_right, "◨").on_hover_text(tr("오른쪽 칸 보이기/숨기기 ( ] )", "Show/hide right panel ( ] )")).clicked() {
                    self.settings.show_right = !self.settings.show_right;
                }
                if ui.selectable_label(self.settings.show_left, "◧").on_hover_text(tr("왼쪽 칸 보이기/숨기기 ( [ )", "Show/hide left panel ( [ )")).clicked() {
                    self.settings.show_left = !self.settings.show_left;
                }
                ui.checkbox(&mut self.settings.auto_advance, tr("점수 후 다음", "Next after scoring")).on_hover_text(tr("크게 보기에서 점수를 주면 다음 그림으로 넘어감", "In the Viewer, scoring a picture moves on to the next one"));
                if self.trash_n > 0 && ui.small_button(trf!("휴지통 비우기 ({})", "Empty Trash ({})", self.trash_n)).clicked() {
                    self.modal = Some(Modal::EmptyTrash { n: self.trash_n });
                }
                if !self.settings.watch_dirs.is_empty() {
                    ui.label(egui::RichText::new(trf!("자동 가져오기 {}곳", "Auto import: {} folders", self.settings.watch_dirs.len())).color(DIM)).on_hover_text(self.settings.watch_dirs.join("\n"));
                }
                if self.sel.len() > 1 {
                    ui.label(egui::RichText::new(trf!("{}장 선택", "{} selected", self.sel.len())).color(ACCENT));
                }
            });
        });
    }
}

enum LeftAction {
    RenameFilter(usize),
    UpdateFilter(usize),
    MoveFilter(usize, isize),
    DeleteFilter(usize),
    ToggleTag(String),
    RenameTag(String),
    DeleteTag(String, usize),
    NewAlbum(u32),
    RenameAlbum(u32),
    DeleteAlbum(u32),
    MoveAlbum(u32, u32),
    /// put the selected pictures into an album (None = out of every album)
    AddToAlbum(Option<u32>),
    /// open in the album view (None = 루트, Some(None) = 무소속)
    OpenAlbum(Option<Option<u32>>),
    /// 태그 여부: (워크플로, 태그없음) on/off
    Kind(bool, bool),
}

/// One row of the album tree: the whole line answers the pointer and shows the
/// selection (like Explorer's folder tree), not just the text. `fold`: Some(folded)
/// draws ▸/▾ in front; the second value is true when that arrow was clicked.
fn tree_row(ui: &mut egui::Ui, indent: f32, text: String, on: bool, fold: Option<bool>, salt: &str) -> (egui::Response, bool) {
    let h = ui.spacing().interact_size.y.max(20.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(40.0), h), Sense::click());
    let v = ui.visuals().clone();
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 3.0, v.selection.bg_fill);
    } else if resp.hovered() {
        p.rect_filled(rect, 3.0, v.widgets.hovered.weak_bg_fill);
    }
    let arrow = Rect::from_min_size(egui::pos2(rect.left() + indent, rect.center().y - 9.0), vec2(18.0, 18.0));
    let mut fold_clicked = false;
    if let Some(folded) = fold {
        let fr = ui.interact(arrow, ui.id().with(("tree_fold", salt)), Sense::click());
        fold_clicked = fr.clicked();
        let col = if fr.hovered() { v.strong_text_color() } else { DIM };
        ui.painter().text(arrow.center(), egui::Align2::CENTER_CENTER, if folded { "▸" } else { "▾" }, egui::TextStyle::Button.resolve(ui.style()), col);
    }
    let col = if on { v.selection.stroke.color } else { v.text_color() };
    ui.painter().text(egui::pos2(arrow.right() + 2.0, rect.center().y), egui::Align2::LEFT_CENTER, text, egui::TextStyle::Button.resolve(ui.style()), col);
    (resp, fold_clicked)
}

fn list_counts(ui: &mut egui::Ui, items: &[(String, usize)], key: &str, q: &str, new_q: &mut Option<String>, _action: &mut Option<LeftAction>) {
    if items.is_empty() {
        ui.label(egui::RichText::new(tr("없음", "none")).color(DIM).small());
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if items.len() > 80 {
            ui.label(egui::RichText::new(trf!("많이 쓰는 80개만 보임 (전체 {}개, 나머지는 검색칸에 {key}:이름)", "Only the 80 most used are shown ({} in total, type {key}:name in the search box for the rest)", items.len())).color(DIM).small());
        }
        let group = query::group_values(q, key);
        for (v, n) in items.iter().take(80) {
            let lv = v.to_lowercase();
            let w = format!("{key}:{v}");
            let on = group.contains(&lv);
            let neg = format!("-{w}");
            let off = query::has_word(q, &neg);
            let r = ui
                .selectable_label(on || off, if off { format!("✕{v} {n}") } else { format!("{v} {n}") })
                .on_hover_text(tr("클릭: 이것만 보기 / Ctrl+클릭: 여러 개 같이 보기 / 오른쪽 클릭: 메뉴", "Click: show only this / Ctrl+click: show several together / right-click: menu"));
            if r.clicked() {
                let ctrl = ui.input(|i| i.modifiers.ctrl);
                let mut g = group.clone();
                if ctrl {
                    if on {
                        g.retain(|x| *x != lv);
                    } else {
                        g.push(lv.clone());
                    }
                } else if on && g.len() == 1 {
                    g.clear();
                } else {
                    g = vec![lv.clone()];
                }
                *new_q = Some(query::set_group(q, key, &g));
            }
            r.context_menu(|ui| {
                if ui.button(tr("이것 빼고 보기", "Show without this")).clicked() {
                    *new_q = Some(query::toggle_word(q, &neg));
                    ui.close();
                }
                if ui.button(if on { tr("같이 보기에서 빼기    Ctrl+클릭", "Remove from combined view    Ctrl+click") } else { tr("같이 보기에 더하기    Ctrl+클릭", "Add to combined view    Ctrl+click") }).clicked() {
                    let mut g = group.clone();
                    if on {
                        g.retain(|x| *x != lv);
                    } else {
                        g.push(lv.clone());
                    }
                    *new_q = Some(query::set_group(q, key, &g));
                    ui.close();
                }
            });
        }
    });
}

pub(super) fn paint_fit(p: &egui::Painter, rect: egui::Rect, t: &egui::TextureHandle) -> egui::Rect {
    let ts = t.size_vec2();
    let scale = (rect.width() / ts.x).min(rect.height() / ts.y);
    let r = egui::Rect::from_center_size(rect.center(), ts * scale);
    p.image(t.id(), r, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
    r
}
