//! Album screen: albums shown like folders in Explorer. The top holds 앨범 없음
//! and the top-level albums; inside an album its sub albums come first, then
//! its pictures, all in the one grid (searched with album:<id>, the top with
//! album:top). Albums are selected, dragged, cut and pasted like pictures.
//! Albums are imgr's own sorting: nothing on disk changes.

use eframe::egui;

use super::*;
use crate::i18n::tr;

enum HeadAct {
    Open(Option<Option<u32>>),
    New(u32),
    Rename(u32),
}

impl App {
    pub(super) fn ui_albums(&mut self, ui: &mut egui::Ui) {
        let cur = self.cur_album();
        // an album that no longer exists (deleted, old search): back to the top
        if let Some(Some(id)) = cur {
            if self.db.album(id).is_none() {
                self.open_album(None);
                return;
            }
        }
        let mut act: Option<HeadAct> = None;

        // path: 🗂 앨범 › A › B          ＋ 새 앨범
        ui.horizontal(|ui| {
            let r = ui.selectable_label(cur.is_none(), egui::RichText::new(tr("🗂 앨범", "🗂 Albums")).strong()).on_hover_text(tr("맨 위 (Ctrl+↑ · Backspace: 한 칸 위)\n앨범을 여기 끌어다 놓으면 맨 위로", "Top level (Ctrl+↑ · Backspace: up one level)\nDrop an album here to move it to the top level"));
            self.mark("album_root", r.rect);
            self.album_drop.push((Some(0), r.rect));
            if r.clicked() {
                act = Some(HeadAct::Open(None));
            }
            match cur {
                Some(Some(id)) => {
                    let chain: Vec<(u32, String)> = self.db.album_chain(id).iter().map(|a| (a.id, a.name.clone())).collect();
                    for (aid, name) in chain {
                        ui.label(egui::RichText::new("›").color(DIM));
                        let r = ui.selectable_label(aid == id, &name);
                        self.album_drop.push((Some(aid), r.rect));
                        if r.clicked() && aid != id {
                            act = Some(HeadAct::Open(Some(Some(aid))));
                        }
                    }
                }
                Some(None) => {
                    ui.label(egui::RichText::new("›").color(DIM));
                    let r = ui.selectable_label(true, tr("무소속", "No album"));
                    self.album_drop.push((None, r.rect));
                }
                None => {}
            }
            ui.add_space(12.0);
            let parent = match cur {
                Some(Some(id)) => Some(id),
                None => Some(0),
                Some(None) => None,
            };
            if let Some(p) = parent {
                let r = ui.button(tr("＋ 새 앨범", "＋ New album")).on_hover_text(if p == 0 { tr("맨 위에 새 앨범", "New album at top level") } else { tr("이 앨범 안에 하위 앨범", "New sub-album in this album") });
                self.mark("album_view_new", r.rect);
                if r.clicked() {
                    act = Some(HeadAct::New(p));
                }
            }
            if let Some(Some(id)) = cur {
                let r = ui.button("✎").on_hover_text(tr("이 앨범 이름 바꾸기", "Rename this album"));
                if r.clicked() {
                    act = Some(HeadAct::Rename(id));
                }
            }
            ui.label(egui::RichText::new(trf!("클릭 열기 · Ctrl+클릭 고르기 · 끌어 놓기 = 옮기기(Ctrl: 복사) · Ctrl+N 새 앨범 · {0} 이 앨범에서 빼기 · {1} 휴지통 · Ctrl+↑ 위로", "Click: open · Ctrl+click: select · Drag and drop = move (Ctrl: copy) · Ctrl+N new album · {0} remove from this album · {1} Trash · Ctrl+↑ up", self.del_keys().0, self.del_keys().1)).color(DIM).small());
        });
        ui.add_space(4.0);

        // cover of each album cell = its newest picture; none right in it: the newest of its sub albums
        let lead = self.album_lead();
        self.album_cover.clear();
        if !lead.is_empty() {
            let mut newest: HashMap<Option<u32>, (i64, String)> = HashMap::new();
            for im in self.db.images.iter().filter(|i| !i.missing && !i.trash) {
                for k in im.albums.iter().map(|&a| Some(a)) {
                    let e = newest.entry(k).or_insert((i64::MIN, String::new()));
                    if im.added > e.0 {
                        *e = (im.added, im.id.clone());
                    }
                }
            }
            for t in &lead {
                let cov = newest.get(t).cloned().or_else(|| {
                    let sub = self.db.album_subtree((*t)?);
                    newest.iter().filter(|(k, _)| k.is_some_and(|x| sub.contains(&x))).max_by_key(|(_, v)| v.0).map(|(_, v)| v.clone())
                });
                if let Some((_, id)) = cov {
                    self.album_cover.insert(*t, id);
                }
            }
        }
        // albums that left the view (deleted, moved) leave the selection
        let here: HashSet<u32> = lead.iter().flatten().copied().collect();
        self.album_sel.retain(|a| here.contains(a));

        self.ui_grid(ui);

        match act {
            Some(HeadAct::Open(a)) => self.open_album(a),
            Some(HeadAct::New(p)) => self.new_album_under(p),
            Some(HeadAct::Rename(id)) => {
                let name = self.album_label(Some(id));
                self.modal = Some(Modal::AlbumRename { id, new: name });
            }
            None => {}
        }
    }
}
