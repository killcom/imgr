# imgr

한국어: [README_ko.md](README_ko.md)

A Windows image manager, made to replace Explorer for large piles of pictures — especially AI-generated ones (ComfyUI) that you keep, score and sort.

## What it does

- **Scores (1–6) and hashtags**: one key each, on many pictures at once.
- **Albums**: imgr's own grouping. A picture can be in several albums, and albums can have sub-albums. Move things like in Explorer: drag and drop, Ctrl+X/C/V.
- **ComfyUI info**: reads the prompt, checkpoint, LoRAs and seed stored in each PNG, shows them and lets you search them.
- **Search**: chain conditions like `#tag score>=4 ckpt:name album:trip+ added:today`. Save searches you use often.
- **Viewer, Compare, Table compare**:
  - Put several pictures side by side and zoom into the same spot together.
  - Compare checkpoints / presets in a table, matched by seed.
- **Import**:
  - Drop files on the window, Ctrl+V, pick a folder, or let imgr watch folders.
  - The same picture (same content, by hash) is imported only once.
- **Export**: copies for uploading as jpg/png with metadata removed, with random names if you like.
- Pictures are known by their content, so scores and tags follow a file when you move or rename it inside the folder.
- **Language**: English / Korean (⚙ Settings; on first start it follows the Windows language).

Full guide: [docs/usage.md](docs/usage.md) · in the program, press F1.

## Download

Get `imgr_v…_win64.zip` from the [Releases](../../releases) page, unzip it into the folder where your pictures should live, and run `imgr.exe`.

- The zip is built from this source by GitHub Actions (`.github/workflows/release.yml`).
- imgr is not code-signed, so Windows may say it protected your PC: **More info › Run anyway**.

## Requirements

- Windows 10 / 11 (64-bit). Windows only (it uses the Explorer clipboard, drag and drop, and the Recycle Bin).
- Korean text uses the Windows font Malgun Gothic.

## Build

1. Install [Rust](https://rustup.rs) (stable) and Visual Studio Build Tools (C++).
2. In this folder, run:
   ```
   cargo build --release
   ```
3. Copy `target\release\imgr.exe` to the folder where your pictures should live, and run it.

## Folder

The folder that holds the exe (or the environment variable `IMGR_ROOT`) is imgr's home:

```
imgr.exe
imgs\00001\…    pictures (2000 per folder, change it in ⚙ Settings)
data\           records: db.json (scores, tags, albums…), settings.json, thumbnail cache, backups
```

- Everything is kept in this folder. Nothing is sent over the internet.
- Script control (for tests and automation) is off at first.
  - To turn it on: ⚙ Settings › "Allow script control", or run with the environment variable `IMGR_CTL=1`.
  - When on, imgr takes commands on `127.0.0.1:47811` from this PC only, and refuses requests from web pages.
  - `tools/imgr_ctl.py` is an example that sends commands.

## Code

| File | What it does |
|---|---|
| `src/db.rs` | records, reading and syncing the folder, import, rename, PNG / ComfyUI info |
| `src/query.rs` | search language, sorting (with unit tests) |
| `src/ops.rs` | export, watched folders |
| `src/winshell.rs` | Win32: file clipboard, drag out, Show in Explorer, single window |
| `src/thumbs.rs` | thumbnail loading and disk cache |
| `src/ctl.rs` | script control (localhost HTTP) |
| `src/i18n.rs` | screen language (Korean / English) |
| `src/app.rs`, `src/app/` | the screen and its actions (top bar, side panels, grid, album view, viewer, compare, dialogs) |

Unit tests: `cargo test`

Release: set the version in `Cargo.toml`, add a `## v0.14` part to `CHANGELOG.md`, then push a tag `v0.14`. GitHub builds, tests and publishes `imgr_v0.14_win64.zip` (`tools/package.ps1` makes the same zip by hand).

## License

imgr is released under the [GNU General Public License v3.0](LICENSE) or any later version (GPL-3.0-or-later).

- Anyone may use, change and share it.
- If you share a changed version, you must publish its source under the same license.

The libraries it uses (egui/eframe, image, serde, …) keep their own licenses (MIT, Apache-2.0, …).
