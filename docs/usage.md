# imgr usage

한국어: [usage_ko.md](usage_ko.md)

The folder that holds imgr.exe is imgr's home. `imgs\` (pictures) and `data\` (records) are created next to it.
To use another location, set the environment variable `IMGR_ROOT` to that folder.

## Starting

```text
Double-click imgr.exe. On start it reads the imgs folder, sorts out new, moved and missing pictures, and shows the list right away.
If imgr is already running, it does not open a new window; it brings the running window to the front (via a Windows signal, no network used).
All pictures are kept in imgs\00001, 00002 … (2000 pictures per folder; change this in ⚙ Settings at the top). imgr fills the folders by itself.
Scores, tags, albums and filters are saved in data\imgr.db (SQLite); only changed pictures are written (data\backup = one copy per day for 14 days, imgr_<date>.db).
A save is all or nothing, so closing imgr in the middle cannot break the file. If imgr.db cannot be read, imgr only reads and does not save (copy a file from data\backup to imgr.db to restore).
The data\db.json of v0.16 and before is moved once when v0.20 first starts and kept as db.json.v016 (older versions no longer open that data).
Pictures are recognized by their content, so if you move or rename files inside imgs, their scores and tags follow.
Window size and position, side panel widths and the last picture you were viewing are restored next time.
```

## Screen

```text
Top: ⚙ Settings · search box · recent searches (▾) · sort · Import · ⟳   (views are Album view / Search view on the left, Viewer = Enter, Compare = C, Table compare = M)
⚙ Settings: Language (한국어 / English, changes as soon as you click) · pictures per folder · Allow script control · clear records of missing pictures
Left: 🗂 Album view (/ = top level · album tree · Trash. You can click anywhere on a row)
      ▦ Search view (Show all · saved searches · score (N/A = no score) · hashtag · album · checkpoint · LoRA · Workflow info · folder) (click a title to collapse it)
  Workflow info: workflow = pictures that contain ComfyUI info (prompt, checkpoint, seed); no workflow = pictures without that info (jpg etc.).
             Both on shows everything, only one on shows only that. At first only workflow is on. Not applied in Album view or Trash.
  (folder = a real folder in imgs, 00001… / album = a grouping used only inside imgr)
  Checkpoint, LoRA, folder: click = show only that one, Ctrl+click = show several together (shown if it matches any of them)
Right: preview of the selected picture · prompt · score · hashtags · info (path = folder/file name inside imgs)
  "Show in Explorer" = opens the folder in imgs that contains the picture and selects it
Drag the borders of the side panels with the mouse to change their width. Dragging the handle (▧) at the bottom-left corner of the right preview
changes the right panel width and the preview height together. [ ] keys = hide/show the left/right panel. F11 = full screen.
```

## Adding pictures

```text
・Drag files or folders onto the window → they are copied into imgs
・Copy files in Explorer (Ctrl+C) → Ctrl+V in imgr
・"＋ Import" above → enter a folder path (subfolders included). Choose "Move" and they are removed from the original folder
・Auto import: add a folder at the bottom of the "＋ Import" window, and while imgr is running it copies new pictures from there every minute
  (e.g. ComfyUI's output folder). When you first add it, pictures already in that folder are brought in too.
  Subfolders starting with data_blind…, _for_kohya… or a dot (.) are not entered.
・If a picture with the same content already exists, it is skipped (the same picture is added only once, even under a different name)
・If only the name matches but the content differs, it is added with a changed name like 'name (2).png'
・If you add while an album is open, the pictures also go into that album (even ones that already existed)
・Files put directly into imgs with Explorer are registered automatically at start, on F5, and every 45 seconds
```

## Viewing and selecting (same as Explorer)

```text
Click = one / Ctrl+click = add or remove / Shift+click = range / Ctrl+A = everything visible
Drag from an unselected picture or an empty area = select several with a box (hold Ctrl or Shift to add to the current selection)
Click an empty area = clear the selection
Arrow keys to move (Shift+arrows = range), PageUp/Down, Home/End
Double-click, Enter, Space = Viewer
Ctrl+wheel or the bar at the bottom right = thumbnail size.  "File name" below = show file names under thumbnails
Drag already selected pictures onto an album (in the left album list or an album on the album screen) to put them in that album (inside Album view this moves them, Ctrl = copy)
Drag out of the window and the files are handed over as they are to Explorer, a browser upload box, etc.
  (for an unselected picture, click it once to select it, then drag. Esc while dragging = cancel)
Right-click = menu (Viewer · Compare · score · tags · album · rename · Export · copy · Show in Explorer …)
```

## Viewer

```text
Wheel = zoom (centered on the mouse), drag = pan, R = fit, Z = actual size (1:1)
← → or the mouse back/forward buttons = previous/next.  Esc = go back
Giving a score 1~6 moves on to the next picture (it stays if you turn off "Next after scoring" below)
Top: file name, score, tags; second line: checkpoint and seed; bottom: key hints.  ` = hide/show this text.
```

## Score (1~6)

```text
Keyboard 1~6, 0 = clear (` also clears, and hides the text in the Viewer only).  Left N/A = only pictures without a score. The number buttons on the right do the same. With several selected, it applies to all.
The circle at the top left of a thumbnail is the score. The score bar on the left = number of pictures per score.
Sort "Random (for rating)" = for scoring without the order influencing you.
```

## Hashtags

```text
Press T, type, then Enter (separate several with commas or spaces). Tab = autocomplete. The input box stays after Enter so you can keep typing.
"Quick tags" buttons on the right = add recent/frequently used tags in one click (if all are already attached, they are removed)
Click a tag chip to search for that tag, the ✕ beside it = remove it. Enter in an empty input box = finish typing.  #2 at the top right of a thumbnail = number of tags.
Right-click a hashtag on the left = search 'without this' · attach/remove on selected pictures · rename (merges if the name already exists) · remove from all
```

## Rename (F2)

```text
One picture: opens with the name selected without the extension → type → Enter
Several pictures: a "name_###" pattern + start number (number of # = digits), or random names for uploading (11 characters, in time order)
If the name already exists, it is not changed and you are told. Double-clicking the file name in the right panel does the same.
```

## Export (Ctrl+E) — copies for uploading

```text
Copies the selected pictures to another folder. For JPG/PNG the ComfyUI prompt info is removed ("Copy original as is" keeps it).
Random names for uploading, and automatically attaching a hashtag (e.g. pixiv) to the exported pictures. Settings are remembered.
```

## Copy and paste

```text
Ctrl+C = copy the selected pictures (can also be pasted into Explorer or an upload window)
Ctrl+X = cut the selected pictures (for moving between albums. The files stay as they are)
Ctrl+V = paste into the open album:  pictures copied with Ctrl+C are also added to that album (to several albums at once)
                                     pictures cut with Ctrl+X are removed from the album they were cut from and moved to this album
         files copied in Explorer are imported into imgs (and also added to the open album, if one is open)
Ctrl+Shift+C = copy the file path as text
```

## Search (the search box at the top, Ctrl+F or /)

```text
#pixiv            has the tag          -#pixiv   does not have the tag
score>=4  score:5  score:0 (no score)
preset:photoB   ckpt:obsess   lora:pastel   seed:12345   folder:00001   name:003640
is:unscored  is:untagged  is:trash  is:missing
album:trip  album:trip+  album:none = pictures directly in the album "trip" / pictures in "trip" and its sub-albums / pictures in no album
                     (clicking an album in Search view puts album:trip+ in the box, album:trip+|album:sea+ = either one.
                      If another album has the same name, use a path like album:trip/sea; if the name has spaces, use quotes like "album:my album".
                      If you rename an album, the name in the search box follows. The old number form album:3 also works)
added:today  added:yesterday  added:3d (last 3 days)  added>=11-01  = date added (today/yesterday also work)
date:2026-11-15  date<2026-01-01  = file date
sort:new / old / name / score / seed / size / random   (if the search has sort:, the sort box shows "(search)")
Conditions are separated by commas: long hair, scar, #pixiv = all must match. a|b = either one.
Any other text finds a whole prompt tag (space = _, case and weights ignored; hashtags and preset names too).
__scar__ / __scar / scar__ = tags containing / ending with / starting with scar. File names: name:, checkpoints: ckpt:
Clicking an item in the left list adds it to or removes it from the search: AND (left of the search box) = a new condition, OR = joined to the last one with | (either).
Ctrl+click = only this of its kind. Right-click = 'without this' (for score, 'or higher').
For frequent searches use "＋ Save current search as filter" on the left. Right-click a saved search = rename · overwrite with current search · reorder · delete.
▾ = reuse a recent search.
```

## Compare (C)

```text
Select 2~12 pictures and press C. They are placed side by side with preset, checkpoint, seed and score shown on top (differing parts are highlighted).
Wheel zoom and drag apply to all cells together → compare parts like eyes or hands at the same position.
Click a cell or use ←→ to choose it, then 1~6 = score for that cell. Double-click a cell = view that picture large. Esc = go back.
```

## Table compare (M) — checkpoint and preset test results at a glance

```text
Rows = seeds (pictures made from the same queue), columns = preset / checkpoint / LoRA combinations (chosen at the top).
Only pictures visible in the current search are used → narrow the range with a search first, then press M.
Click a cell, move with arrow keys, 1~6 score, double-click = view large. Right-click a column title = hide that column.
```

## Albums (A) — grouping used only inside imgr (the real folders and files in imgs stay as they are)

```text
One picture can be in several albums at once. You can also make sub-albums inside an album.
"No album" = pictures that are in no album. Pictures never go there automatically. Adding, removing and moving can be undone with Ctrl+Z.
There are two ways to view:
  🗂 Album view = see albums as folder cells, like Explorer (A, / and the albums in the left Album view)
  ▦ Search view = search across all pictures (Esc, "Show all" on the left). "Album" in the left Search view works like checkpoint:
              click = only that album (including pictures of its sub-albums), Ctrl+click = several albums together (pictures of all chosen albums are shown), click again when only one is chosen = remove it.
              It filters together with other filters (score, tag, checkpoint…)
Album view:
  The top level (/) shows the top-level albums, then the pictures put at the top and those in no album (No album).
  The top holds pictures like an album: a picture in Album1 can sit at the top too. Inside an album, sub-album cells come first, then that album's pictures. Clicking the gap between cells does nothing.
  New album: Ctrl+N, or right-click an empty area → "New album…" (inside an album it makes a sub-album, at the top level a top-level album).
           It is created only when you enter a name and press 'Create' (if you cancel, nothing is created). "＋ New album" at the top does the same.
  Click an album cell = open it right away.
  Selecting: albums with Ctrl+click, Shift+click, dragging a box in an empty area, or right-click (pictures and albums can be selected together).
          Arrow keys, PageUp/Down, Home/End also move through album cells and pictures as one line (Shift = range). Enter on No album = open it.
  Right-click: album = Open · Sub-album · Rename · Cut · Delete / picture = the picture menu (including "Remove from 'album'")
  Enter = open the selected album.  F2 = rename.
  Del: selected pictures = removed from this album only (those in other albums stay there).
       If this album is the picture's last album, it goes to the imgr Trash (restore with Del in is:trash or Ctrl+Z).
       selected albums = delete the album (after asking). Only the album disappears, the pictures stay, and sub-albums move up one level.
  Shift+Del: selected pictures = to the imgr Trash (gone from every album; Ctrl+Z or Restore puts them back in their albums).
        selected albums = delete them with their sub-albums and pictures (after asking). Pictures go to the imgr Trash; the albums do not come back.
  "Swap Del and Shift+Del" in Settings swaps the two keys.
  Moving: drag pictures or albums onto another album (cell · path at the top · left Album view) = they leave this album and move there
          (for pictures, dropping while holding Ctrl copies = they stay in this album too. Album cells can be dragged right away without selecting them)
          or Ctrl+X → open the destination and Ctrl+V.  The top level works the same: dropping on "🗂 Album" at the top or / on the left = move to the top (Ctrl = copy), Ctrl+V at the top = put them at the top too.
  Adding: drag the selected pictures onto an album / Ctrl+C → open an album and Ctrl+V / right-click a picture → album
  Ctrl+↑ · Alt+↑ · Backspace · Esc · mouse back button = up one level.  You can also click the path at the top (Album › trip › sea).
  When you select a picture, it is shown large at the top of the right panel.
  Typing in the search box in Album view, or clicking a filter in Search view, switches to Search view.
"Album" in the right info panel = the albums the picture is in (click to open that album).
Preset values themselves are read only from file names of the form g_<preset>_<number> (setting them by hand was removed) and are used only in Table compare and Compare.
```

## Deleting

```text
Del = to the imgr Trash (hidden from the list, the file stays). Ctrl+Z to undo.
Left Album view's "Trash" (or is:trash) shows the Trash; Del there = restore. Right-click the Trash row = empty it.
"Empty Trash" below = sends those files to the Windows Recycle Bin (you can restore them from the Recycle Bin).
is:missing = records of files that disappeared from imgs (only those that had a score, tag or album). Clear the records in ⚙ Settings.
```

## Other

```text
F1 = search syntax and shortcuts.  F5 = re-read imgs (also read automatically every 45 seconds)
Ctrl+Z = undo score, tag, album and Trash changes
```

## Remote control (for scripts and testing)

```text
Off at first. Turn on ⚙ Settings "Allow script control" and from the next start imgr accepts commands on 127.0.0.1:47811, only from within this PC
(the port is in data\ctl.port, web page requests are rejected). Setting the environment variable IMGR_CTL=1 also opens it, for that run only.
With tools\imgr_ctl.py you can send search, selection, score, tag, screen capture and mouse/key input, and every command is processed in the same order as the user's clicks.
```
