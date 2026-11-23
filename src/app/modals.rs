//! Dialog windows.

use std::path::PathBuf;

use eframe::egui::{self, Key};

use super::*;
use crate::i18n::tr;

fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// "alice_###" + 7 -> "alice_007"
fn numbered(pattern: &str, n: u32) -> String {
    let hashes = pattern.chars().filter(|c| *c == '#').count();
    if hashes == 0 {
        return format!("{pattern}_{n}");
    }
    let start = pattern.find('#').unwrap();
    let end = pattern.rfind('#').unwrap() + 1;
    format!("{}{:0w$}{}", &pattern[..start], n, &pattern[end..], w = hashes)
}

impl App {
    pub(super) fn ui_modal(&mut self, ctx: &egui::Context) {
        let Some(m) = self.modal.take() else { return };
        let enter = ctx.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
        let esc = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut keep = true;
        let m = match m {
            Modal::Import { mut dir, mut mv, mut watch_new } => {
                let mut go = false;
                let mut remove: Option<usize> = None;
                egui::Modal::new(egui::Id::new("import")).show(ctx, |ui| {
                    ui.set_width(600.0);
                    ui.heading(tr("가져오기", "Import"));
                    ui.label(tr("폴더(하위 폴더 포함) 안 그림을 imgs로 넣습니다. 내용이 같은 그림이 이미 있으면 건너뜁니다.", "Copies the pictures in a folder (including subfolders) into imgs. Pictures that already exist with identical content are skipped."));
                    ui.add(egui::TextEdit::singleline(&mut dir).hint_text(tr(r"예) D:\ComfyUI\output\my_pictures", r"e.g. D:\ComfyUI\output\my_pictures")).desired_width(f32::INFINITY));
                    ui.checkbox(&mut mv, tr("복사 대신 옮기기 (원래 폴더에서 없어짐)", "Move instead of copy (removed from the original folder)"));
                    ui.horizontal(|ui| {
                        let watch_focused = ui.ctx().memory(|m| m.has_focus(egui::Id::new("watch_new")));
                        if ui.button(tr("가져오기", "Import")).clicked() || (enter && !watch_focused && !dir.trim().is_empty()) {
                            go = true;
                        }
                        if ui.button(tr("닫기", "Close")).clicked() || esc {
                            keep = false;
                        }
                    });
                    ui.label(egui::RichText::new(tr("창에 파일·폴더를 끌어다 놓거나, 탐색기에서 복사(Ctrl+C) 후 여기서 Ctrl+V 해도 넣을 수 있습니다.", "You can also drag files or folders onto the window, or copy them in Explorer (Ctrl+C) and press Ctrl+V here.")).color(DIM).small());
                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(egui::RichText::new(tr("자동 가져오기 (imgr가 켜져 있는 동안 1분마다 새 그림을 복사해 옴)", "Auto import (while imgr is running, new pictures are copied every minute)")).strong());
                    for (k, d) in self.settings.watch_dirs.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button("✕").on_hover_text(tr("자동 가져오기 그만", "Stop auto import")).clicked() {
                                remove = Some(k);
                            }
                            ui.label(d);
                        });
                    }
                    ui.horizontal(|ui| {
                        let wr = ui.add(egui::TextEdit::singleline(&mut watch_new).id(egui::Id::new("watch_new")).hint_text(tr("폴더 경로", "Folder path")).desired_width(440.0));
                        let add = ui.button(tr("추가", "Add")).clicked() || (wr.lost_focus() && enter);
                        let d = watch_new.trim().trim_matches('"').to_string();
                        if add && !d.is_empty() && !PathBuf::from(&d).is_dir() {
                            self.notice(trf!("폴더가 없습니다: {d}", "Folder not found: {d}"), true);
                        } else if add && !d.is_empty() {
                            if !self.settings.watch_dirs.contains(&d) {
                                self.settings.watch_dirs.push(d);
                                self.last_watch = Instant::now() - WATCH_EVERY;
                            }
                            watch_new.clear();
                        }
                    });
                    ui.checkbox(&mut self.settings.watch_move, tr("자동 가져오기도 옮기기로 (원래 폴더에서 없어짐)", "Auto import also moves (removed from the original folder)"));
                    ui.label(egui::RichText::new(tr("처음 추가할 때 그 폴더에 이미 있는 그림도 모두 들어옵니다.", "When first added, pictures already in that folder are imported too.")).color(DIM).small());
                });
                if let Some(k) = remove {
                    self.settings.watch_dirs.remove(k);
                }
                if go {
                    let p = PathBuf::from(dir.trim().trim_matches('"'));
                    if p.exists() {
                        self.settings.import_dir = dir.clone();
                        self.settings.import_move = mv;
                        self.start_import(vec![p], mv);
                        keep = false;
                    } else {
                        self.notice(tr("그 폴더가 없습니다", "That folder does not exist"), true);
                    }
                }
                Modal::Import { dir, mv, watch_new }
            }
            Modal::SaveFilter { mut name } => {
                let mut go = false;
                egui::Modal::new(egui::Id::new("savef")).show(ctx, |ui| {
                    ui.heading(tr("필터로 저장", "Save as filter"));
                    ui.label(egui::RichText::new(&self.query_text).color(ACCENT));
                    let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("이름 (예: pixiv 올림 · photoB 5점+)", "Name (e.g. pixiv posted · photoB 5+)")).desired_width(320.0));
                    if !r.has_focus() {
                        r.request_focus();
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("저장", "Save")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go && !name.trim().is_empty() {
                    // kept with its OR spelled out, so it means the same in AND mode
                    let q = query::explicit(&self.query_text, self.settings.search_or);
                    if let Some(f) = self.db.saved.iter_mut().find(|f| f.name == name.trim()) {
                        f.query = q;
                    } else {
                        self.db.saved.push(db::SavedFilter { name: name.trim().to_string(), query: q });
                    }
                    self.dirty = true;
                    keep = false;
                }
                Modal::SaveFilter { name }
            }
            Modal::RenameFilter { idx, mut name } => {
                let mut go = false;
                egui::Modal::new(egui::Id::new("renf")).show(ctx, |ui| {
                    ui.heading(tr("필터 이름 바꾸기", "Rename filter"));
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(320.0));
                    if !r.has_focus() {
                        // once only: asking every frame cuts off Korean (IME) typing
                        r.request_focus();
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("바꾸기", "Rename")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go && !name.trim().is_empty() && idx < self.db.saved.len() {
                    self.db.saved[idx].name = name.trim().to_string();
                    self.dirty = true;
                    keep = false;
                }
                Modal::RenameFilter { idx, name }
            }
            Modal::EmptyTrash { n } => {
                egui::Modal::new(egui::Id::new("empty")).show(ctx, |ui| {
                    ui.heading(tr("휴지통 비우기", "Empty Trash"));
                    ui.label(trf!("imgr 휴지통의 {n}장을 윈도우 휴지통으로 보냅니다.\n(윈도우 휴지통에서는 아직 되살릴 수 있습니다)", "Sends the {n} pictures in the imgr Trash to the Windows Recycle Bin.\n(They can still be restored from the Recycle Bin)"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("보내기", "Send")).clicked() {
                            self.empty_trash();
                            keep = false;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                Modal::EmptyTrash { n }
            }
            Modal::ForgetMissing { ids } => {
                let n = ids.len();
                egui::Modal::new(egui::Id::new("forget")).show(ctx, |ui| {
                    ui.heading(tr("사라진 그림 기록 지우기", "Clear records of missing pictures"));
                    ui.label(trf!("imgs에서 파일이 없어진 {n}장의 기록(점수·태그)을 지웁니다. 파일이 돌아와도 점수·태그는 돌아오지 않습니다.", "Clears the records (scores, tags) of {n} pictures whose files are gone from imgs. If the files come back, their scores and tags will not."));
                    ui.horizontal(|ui| {
                        if ui.button(tr("지우기", "Clear")).clicked() {
                            self.forget_missing(&ids);
                            keep = false;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                Modal::ForgetMissing { ids }
            }
            Modal::Help => {
                let r = egui::Modal::new(egui::Id::new("help")).show(ctx, |ui| {
                    ui.set_width(720.0);
                    ui.heading(tr("검색 문법 · 단축키", "Search syntax · Shortcuts"));
                    egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.75).show(ui, |ui| {
                        ui.label(egui::RichText::new(crate::app::help_text()).monospace());
                    });
                    if ui.button(tr("닫기", "Close")).clicked() || esc || enter {
                        keep = false;
                    }
                });
                if r.should_close() {
                    keep = false;
                }
                Modal::Help
            }
            Modal::Rename { ids, mut name, mut fresh, mut upload, mut pattern, mut start, mut error } => {
                let mut go = false;
                let one = ids.len() == 1;
                egui::Modal::new(egui::Id::new("rename")).show(ctx, |ui| {
                    ui.set_width(520.0);
                    if one {
                        ui.heading(tr("이름 바꾸기", "Rename"));
                        let id = egui::Id::new("rename_edit");
                        let r = ui.add(egui::TextEdit::singleline(&mut name).id(id).desired_width(f32::INFINITY));
                        if fresh {
                            // select the name without the extension, like Explorer
                            r.request_focus();
                            let stem_len = split_ext(&name).0.chars().count();
                            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(stem_len))));
                                st.store(ui.ctx(), id);
                            }
                            fresh = false;
                        }
                        ui.checkbox(&mut upload, tr("업로드용 무작위 이름으로 (11자, 시간순)", "Random name for upload (11 chars, in time order)"));
                    } else {
                        ui.heading(trf!("{}장 이름 한꺼번에 바꾸기", "Rename {} pictures", ids.len()));
                        ui.radio_value(&mut upload, false, tr("번호 붙이기", "Add numbers"));
                        ui.add_enabled_ui(!upload, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(tr("이름 틀", "Name pattern"));
                                ui.add(egui::TextEdit::singleline(&mut pattern).desired_width(220.0));
                                ui.label(tr("시작", "Start"));
                                ui.add(egui::DragValue::new(&mut start).range(0..=999999));
                            });
                            ui.label(egui::RichText::new(tr("### 자리에 번호가 들어갑니다 (# 개수 = 자릿수). 확장자는 그대로.", "The number goes in place of ### (number of # = digits). The extension stays.")).color(DIM).small());
                        });
                        ui.radio_value(&mut upload, true, tr("업로드용 무작위 이름 (11자, 시간순)", "Random name for upload (11 chars, in time order)"));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(tr("미리 보기", "Preview")).color(DIM));
                        for (k, id) in ids.iter().take(3).enumerate() {
                            if let Some(im) = self.img(id) {
                                let (_, ext) = split_ext(im.name());
                                let new = if upload { trf!("(무작위){ext}", "(random){ext}") } else { format!("{}{ext}", numbered(&pattern, start + k as u32)) };
                                ui.label(format!("{}  →  {}", im.name(), new));
                            }
                        }
                        if ids.len() > 3 {
                            ui.label(egui::RichText::new("…").color(DIM));
                        }
                    }
                    if !error.is_empty() {
                        ui.label(egui::RichText::new(&error).color(ERR));
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("바꾸기", "Rename")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go {
                    let mut names: Vec<(String, String)> = Vec::new();
                    for (k, id) in ids.iter().enumerate() {
                        let Some(im) = self.img(id) else { continue };
                        let (_, ext) = split_ext(im.name());
                        let new = if upload {
                            format!("{}{ext}", db::upload_name(k as u64))
                        } else if one {
                            // keep the picture an image: no extension -> keep the old one
                            let n = name.trim().to_string();
                            let (_, new_ext) = split_ext(&n);
                            if new_ext.is_empty() || !db::is_image(std::path::Path::new(&n)) { format!("{n}{ext}") } else { n }
                        } else {
                            format!("{}{ext}", numbered(&pattern, start + k as u32))
                        };
                        names.push((id.clone(), new));
                    }
                    let bad = names.iter().find_map(|(_, n)| db::bad_name(n));
                    if let Some(b) = bad {
                        error = b.to_string();
                    } else {
                        match self.rename_images(names) {
                            Ok(n) => {
                                self.notice(trf!("{n}장 이름 바꿈", "Renamed {n} pictures"), false);
                                keep = false;
                            }
                            Err(e) => error = e,
                        }
                    }
                }
                Modal::Rename { ids, name, fresh, upload, pattern, start, error }
            }
            Modal::Export { ids, mut dir, mut fmt, mut quality, mut upload, mut tag } => {
                let mut go = false;
                egui::Modal::new(egui::Id::new("export")).show(ctx, |ui| {
                    ui.set_width(560.0);
                    ui.heading(trf!("{}장 내보내기", "Export {} pictures", ids.len()));
                    ui.label(tr("다른 폴더에 복사본을 만듭니다 (imgs 원본은 그대로).", "Makes copies in another folder (originals in imgs stay)."));
                    ui.add(egui::TextEdit::singleline(&mut dir).hint_text(tr(r"보낼 폴더 예) D:\upload\pixiv", r"Target folder, e.g. D:\upload\pixiv")).desired_width(f32::INFINITY));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut fmt, "jpg".to_string(), tr("JPG (프롬프트 정보 지움)", "JPG (prompt info removed)"));
                        ui.radio_value(&mut fmt, "png".to_string(), tr("PNG (프롬프트 정보 지움)", "PNG (prompt info removed)"));
                        ui.radio_value(&mut fmt, "keep".to_string(), tr("원본 그대로 복사", "Copy original as is"));
                    });
                    if fmt == "jpg" {
                        ui.add(egui::Slider::new(&mut quality, 70..=100).text(tr("JPG 품질", "JPG quality")));
                    }
                    if fmt == "keep" {
                        ui.label(egui::RichText::new(tr("원본 복사는 ComfyUI 프롬프트가 파일 안에 남습니다.", "Copying the original keeps the ComfyUI prompt inside the file.")).color(ERR).small());
                    }
                    ui.checkbox(&mut upload, tr("업로드용 무작위 이름 (11자, 시간순)", "Random name for upload (11 chars, in time order)"));
                    ui.horizontal(|ui| {
                        ui.label(tr("내보낸 그림에 해시태그 붙이기", "Add hashtag to exported pictures"));
                        ui.add(egui::TextEdit::singleline(&mut tag).hint_text(tr("예) pixiv", "e.g. pixiv")).desired_width(160.0));
                    });
                    ui.horizontal(|ui| {
                        if ui.button(tr("내보내기", "Export")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go {
                    let d = PathBuf::from(dir.trim().trim_matches('"'));
                    if dir.trim().is_empty() {
                        self.notice(tr("보낼 폴더를 적어 주세요", "Enter a folder to export to"), true);
                    } else if db::path_inside(&d, &db::src_dir()) {
                        self.notice(tr("imgs 안으로는 내보낼 수 없습니다", "Cannot export into imgs"), true);
                    } else {
                        let f = match fmt.as_str() {
                            "png" => ops::Fmt::Png,
                            "keep" => ops::Fmt::Keep,
                            _ => ops::Fmt::Jpg(quality),
                        };
                        self.settings.export_dir = dir.trim().to_string();
                        self.settings.export_fmt = fmt.clone();
                        self.settings.export_quality = quality;
                        self.settings.export_upload_names = upload;
                        self.settings.export_tag = tag.clone();
                        self.start_export(ids.clone(), d, f, upload, tag.clone());
                        keep = false;
                    }
                }
                Modal::Export { ids, dir, fmt, quality, upload, tag }
            }
            Modal::TagRename { old, mut new } => {
                let mut go = false;
                egui::Modal::new(egui::Id::new("tagren")).show(ctx, |ui| {
                    ui.heading(trf!("#{old} 이름 바꾸기", "Rename #{old}"));
                    ui.label(tr("이미 있는 태그 이름을 적으면 그 태그에 합쳐집니다.", "If you enter an existing tag name, they are merged into it."));
                    let r = ui.add(egui::TextEdit::singleline(&mut new).desired_width(280.0));
                    if !r.has_focus() {
                        // once only: asking every frame cuts off Korean (IME) typing
                        r.request_focus();
                    }
                    let target = clean_tag(&new);
                    if let Some((_, n)) = self.tag_counts.iter().find(|(t, _)| *t == target && target != old) {
                        ui.label(egui::RichText::new(trf!("#{target} ({n}장)에 합칩니다", "Merges into #{target} ({n} pictures)")).color(ACCENT));
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("바꾸기", "Rename")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go {
                    self.rename_tag_all(&old, &new);
                    keep = false;
                }
                Modal::TagRename { old, new }
            }
            Modal::AlbumRename { id, mut new } => {
                let mut go = false;
                let parent = self.db.album(id).map(|a| a.parent).unwrap_or(0);
                egui::Modal::new(egui::Id::new("albumren")).show(ctx, |ui| {
                    let path = self.db.album_path(id);
                    ui.heading(tr("앨범 이름", "Album name"));
                    ui.label(egui::RichText::new(path).color(DIM));
                    let r = ui.add(egui::TextEdit::singleline(&mut new).id(egui::Id::new("albumren_edit")).desired_width(280.0));
                    if !r.has_focus() {
                        // once only: asking every frame cuts off Korean (IME) typing
                        r.request_focus();
                    }
                    let want = new.trim().to_string();
                    let real = self.db.free_album_name(parent, &want, id);
                    if !want.is_empty() && real != want {
                        ui.label(egui::RichText::new(trf!("같은 곳에 '{want}'가 있어서 '{real}'(으)로 됩니다", "'{want}' already exists there, so it becomes '{real}'")).color(ACCENT));
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("정하기", "OK")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go && !new.trim().is_empty() {
                    if let Some(n) = self.db.rename_album(id, &new) {
                        self.undo_barrier(tr("앨범 이름 바꾸기", "Rename album"));
                        self.dirty = true;
                        self.notice(trf!("앨범 이름: {n}", "Album name: {n}"), false);
                    }
                    keep = false;
                }
                Modal::AlbumRename { id, new }
            }
            Modal::AlbumNew { parent, mut new, put } => {
                let mut go = false;
                egui::Modal::new(egui::Id::new("albumnew")).show(ctx, |ui| {
                    ui.heading(tr("새 앨범", "New album"));
                    let place = if parent == 0 { tr("맨 위", "Top level").to_string() } else { self.db.album_path(parent) };
                    ui.label(egui::RichText::new(trf!("만들 곳: {place}", "Location: {place}")).color(DIM));
                    if !put.is_empty() {
                        ui.label(trf!("고른 그림 {}장을 이 앨범에 넣습니다", "Puts the {} selected pictures into this album", put.len()));
                    }
                    let r = ui.add(egui::TextEdit::singleline(&mut new).id(egui::Id::new("albumnew_edit")).hint_text(tr("새 앨범", "New album")).desired_width(280.0));
                    if !r.has_focus() {
                        // once only: asking every frame cuts off Korean (IME) typing
                        r.request_focus();
                    }
                    let want = if new.trim().is_empty() { "새 앨범".to_string() } else { new.trim().to_string() };
                    let real = self.db.free_album_name(parent, &want, 0);
                    if real != want {
                        ui.label(egui::RichText::new(trf!("같은 곳에 '{want}'가 있어서 '{real}'(으)로 됩니다", "'{want}' already exists there, so it becomes '{real}'")).color(ACCENT));
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("만들기", "Create")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go {
                    let name = if new.trim().is_empty() { "새 앨범".to_string() } else { new.trim().to_string() };
                    let id = self.db.new_album(&name, parent);
                    if parent != 0 {
                        self.album_folded.remove(&parent);
                    }
                    self.mark_changed();
                    if put.is_empty() {
                        self.notice(trf!("앨범 '{}' 만듦", "Album '{}' created", self.album_label(Some(id))), false);
                        self.refilter(true);
                    } else {
                        self.add_to_album(&put, Some(id));
                    }
                    keep = false;
                }
                Modal::AlbumNew { parent, new, put }
            }
            Modal::AlbumDelete { mut ids, with_pics, pics } => {
                let mut go = false;
                ids.retain(|&id| self.db.album(id).is_some());
                if ids.is_empty() {
                    keep = false;
                }
                let names: Vec<String> = ids.iter().map(|&id| self.db.album_path(id)).collect();
                let n: usize = ids.iter().map(|id| self.album_counts.get(id).copied().unwrap_or(0)).sum();
                // with_pics: every picture in these albums or their sub albums (each once)
                let tree: std::collections::HashSet<u32> = ids.iter().flat_map(|&id| self.db.album_subtree(id)).collect();
                let n_all = self.db.images.iter().filter(|i| !i.missing && !i.trash && i.albums.iter().any(|a| tree.contains(a))).count();
                let kids: usize = ids.iter().map(|&id| self.db.album_children(id).iter().filter(|k| !ids.contains(&k.id)).count()).sum();
                egui::Modal::new(egui::Id::new("albumdel")).show(ctx, |ui| {
                    if names.len() == 1 {
                        ui.heading(trf!("앨범 '{}' 지우기", "Delete album '{}'", names[0]));
                    } else {
                        ui.heading(trf!("앨범 {}개 지우기", "Delete {} albums", names.len()));
                        for nm in names.iter().take(8) {
                            ui.label(egui::RichText::new(format!("· {nm}")).color(DIM));
                        }
                        if names.len() > 8 {
                            ui.label(egui::RichText::new(trf!("… 외 {}개", "… and {} more", names.len() - 8)).color(DIM));
                        }
                    }
                    if !pics.is_empty() {
                        let what = if with_pics { tr("imgr 휴지통으로", "go to the imgr Trash") } else { tr("이 앨범에서 빠짐", "leave this album") };
                        ui.label(trf!("같이 고른 그림 {}장도 {what}.", "The {} pictures selected with them also {what}.", pics.len()));
                    }
                    if with_pics {
                        let subs = tree.len() - ids.len();
                        ui.label(egui::RichText::new(trf!("앨범{0}과 그 안의 그림 {n_all}장을 지웁니다. 그림은 imgr 휴지통으로 가서 다른 앨범에서도 안 보입니다.", "Deletes the album{0} and the {n_all} pictures in it. The pictures go to the imgr Trash and disappear from other albums too.", if subs > 0 { trf!(" (하위 앨범 {subs}개 포함)", "s (with {subs} sub-albums)") } else { String::new() })).color(ERR));
                        ui.label(egui::RichText::new(tr("그림은 Ctrl+Z 또는 휴지통에서 되살릴 수 있지만, 지운 앨범은 돌아오지 않습니다.", "Pictures can be restored with Ctrl+Z or from the Trash, but the deleted albums do not come back.")).color(DIM).small());
                    } else {
                        ui.label(trf!("앨범만 없어집니다. 그림 {n}장은 지워지지 않고 앨범에서만 빠집니다.", "Only the album is removed. The {n} pictures are not deleted, just taken out of the album."));
                        if kids > 0 {
                            ui.label(trf!("하위 앨범 {kids}개는 한 칸 위로 올라갑니다.", "The {kids} sub-albums move up one level."));
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("지우기", "Clear")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go && !pics.is_empty() {
                    self.delete_pictures_here(&pics, with_pics);
                }
                if go && with_pics {
                    let (na, np) = self.delete_albums_with_pictures(&ids);
                    self.notice(trf!("앨범 {na}개 지움, 그림 {np}장 imgr 휴지통으로 (그림은 Ctrl+Z 되돌리기)", "Deleted {na} albums, {np} pictures to the imgr Trash (Ctrl+Z restores the pictures)"), false);
                    keep = false;
                } else if go {
                    for &id in &ids {
                        self.delete_album(id);
                    }
                    if ids.len() > 1 {
                        self.notice(trf!("앨범 {}개 지움 (그림 {n}장은 그대로)", "Deleted {} albums ({n} pictures kept)", ids.len()), false);
                    }
                    keep = false;
                }
                Modal::AlbumDelete { ids, with_pics, pics }
            }
            Modal::TagDelete { tag, n } => {
                egui::Modal::new(egui::Id::new("tagdel")).show(ctx, |ui| {
                    ui.heading(trf!("#{tag} 떼기", "Remove #{tag}"));
                    ui.label(trf!("{n}장 모두에서 #{tag}를 뗍니다. (Ctrl+Z로 되돌릴 수 있음)", "Removes #{tag} from all {n} pictures. (Undo with Ctrl+Z)"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("떼기", "Remove")).clicked() {
                            self.delete_tag_all(&tag);
                            keep = false;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                Modal::TagDelete { tag, n }
            }
            Modal::Settings { mut per_folder, mut ctl } => {
                let mut go = false;
                let mut forget: Option<Vec<String>> = None;
                egui::Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.heading(tr("설정", "Settings"));
                    ui.add_space(4.0);
                    // language: takes effect at once (both names in their own language)
                    ui.horizontal(|ui| {
                        ui.label("언어 / Language");
                        for (code, name, mark) in [("ko", "한국어", "set_lang_ko"), ("en", "English", "set_lang_en")] {
                            let r = ui.selectable_label(self.settings.lang == code, name);
                            self.mark(mark, r.rect);
                            if r.clicked() && self.settings.lang != code {
                                self.settings.lang = code.to_string();
                                crate::i18n::set(code);
                                self.settings.save();
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(tr("폴더 하나에 넣는 그림 수", "Pictures per folder"));
                        let r = ui.add(egui::TextEdit::singleline(&mut per_folder).id(egui::Id::new("set_per_folder")).desired_width(80.0));
                        self.mark("set_per_folder", r.rect);
                    });
                    ui.label(
                        egui::RichText::new(tr("가져온 그림은 imgs\\00001 같은 번호 폴더에 들어가고, 이 수만큼 차면 다음 번호 폴더(00002 …)에 넣기 시작합니다. 이미 넣은 그림은 옮기지 않습니다.", "Imported pictures go into numbered folders like imgs\\00001; when one holds this many, the next folder (00002 …) is started. Pictures already placed are not moved."))
                            .color(DIM)
                            .small(),
                    );
                    ui.add_space(8.0);
                    ui.separator();
                    let r = ui.checkbox(&mut self.settings.del_swap, tr("Del 과 Shift+Del 을 서로 바꾸기", "Swap Del and Shift+Del"));
                    self.mark("set_del_swap", r.rect);
                    ui.label(
                        egui::RichText::new(if self.settings.del_swap {
                            tr("지금: Del = imgr 휴지통 (모든 앨범에서 안 보임) · 앨범은 그림까지 / Shift+Del = 이 앨범에서만 빼기 · 앨범만 지우기", "Now: Del = imgr Trash (gone from every album) · albums with their pictures / Shift+Del = out of this album only · album only")
                        } else {
                            tr("지금: Del = 이 앨범에서만 빼기 · 앨범만 지우기 / Shift+Del = imgr 휴지통 (모든 앨범에서 안 보임) · 앨범은 그림까지", "Now: Del = out of this album only · album only / Shift+Del = imgr Trash (gone from every album) · albums with their pictures")
                        })
                        .color(DIM)
                        .small(),
                    );
                    ui.add_space(8.0);
                    ui.separator();
                    let r = ui.checkbox(&mut ctl, tr("스크립트 조작 허용 (이 PC 안에서만, 127.0.0.1)", "Allow script control (this PC only, 127.0.0.1)"));
                    self.mark("set_ctl", r.rect);
                    ui.label(
                        egui::RichText::new(trf!(
                            "다른 프로그램(tools\\imgr_ctl.py 등)이 imgr에 명령을 보낼 수 있게 함. 웹 페이지 요청은 거절. 다음에 켤 때부터 적용.{}",
                            "Lets other programs (tools\\imgr_ctl.py etc.) send commands to imgr. Web page requests are refused. Applies from the next start.{}",
                            match self.ctl_port {
                                Some(p) => trf!(" (지금 켜져 있음: {p})", " (currently on: {p})"),
                                None => tr(" (지금 꺼져 있음)", " (currently off)").to_string(),
                            }
                        ))
                        .color(DIM)
                        .small(),
                    );
                    if self.missing_n > 0 {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.label(trf!("imgs에서 없어진 그림의 기록 {}장 (파일이 돌아오면 점수·태그도 돌아옴)", "Records of {} pictures missing from imgs (scores and tags return if the files come back)", self.missing_n));
                        if ui.button(tr("사라진 그림 기록 지우기…", "Clear records of missing pictures…")).clicked() {
                            let ids: Vec<String> = self.db.images.iter().filter(|i| i.missing).map(|i| i.id.clone()).collect();
                            forget = Some(ids);
                        }
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(tr("정하기", "OK")).clicked() || enter {
                            go = true;
                        }
                        if ui.button(tr("취소", "Cancel")).clicked() || esc {
                            keep = false;
                        }
                    });
                });
                if go {
                    match per_folder.trim().parse::<usize>() {
                        Ok(n) if (1..=1_000_000).contains(&n) => {
                            self.settings.per_folder = n;
                            db::set_per_folder(n);
                            let ctl_note = if ctl != self.settings.ctl { tr(" · 스크립트 조작은 다음에 켤 때부터", " · script control applies from the next start") } else { "" };
                            self.settings.ctl = ctl;
                            self.notice(trf!("폴더 하나에 {n}장씩 넣습니다{ctl_note}", "{n} pictures per folder{ctl_note}"), false);
                            keep = false;
                        }
                        _ => self.notice(tr("폴더 하나에 넣는 그림 수는 1 ~ 1000000 사이 숫자로", "Pictures per folder must be a number from 1 to 1000000"), true),
                    }
                }
                if let Some(ids) = forget {
                    // the confirmation takes the place of the settings
                    self.modal = Some(Modal::ForgetMissing { ids });
                    return;
                }
                Modal::Settings { per_folder, ctl }
            }
        };
        if keep {
            self.modal = Some(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rename_helpers() {
        assert_eq!(numbered("alice_###", 7), "alice_007");
        assert_eq!(numbered("a", 3), "a_3");
        assert_eq!(split_ext("x.y.png"), ("x.y", ".png"));
        assert_eq!(split_ext(".hidden"), (".hidden", ""));
    }
}
