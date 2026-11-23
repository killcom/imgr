//! Small Windows shell helpers: file clipboard (like Explorer's Ctrl+C/V),
//! dragging files out to other programs, showing several files selected in
//! Explorer, and reading old cp949 text files.

use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED, IDataObject};
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, ILFree, IShellItemArray, SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHOpenFolderAndSelectItems, SHParseDisplayName,
};
use windows::core::{HSTRING, PCWSTR};

/// Put files on the clipboard so they can be pasted in Explorer, a browser upload box, etc.
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    let v: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    let _clip = clipboard_win::Clipboard::new_attempts(10).map_err(|e| e.to_string())?;
    clipboard_win::raw::set_file_list(&v).map_err(|e| e.to_string())
}

/// Files currently on the clipboard (after Ctrl+C in Explorer), if any.
pub fn clipboard_files() -> Vec<PathBuf> {
    clipboard_win::get_clipboard::<Vec<PathBuf>, _>(clipboard_win::formats::FileList).unwrap_or_default()
}

pub fn clipboard_text() -> Option<String> {
    clipboard_win::get_clipboard_string().ok()
}

pub fn copy_text(s: &str) -> Result<(), String> {
    clipboard_win::set_clipboard_string(s).map_err(|e| e.to_string())
}

struct Pidls(Vec<*mut ITEMIDLIST>);
impl Drop for Pidls {
    fn drop(&mut self) {
        for p in &self.0 {
            unsafe { ILFree(Some(*p as *const _)) };
        }
    }
}

fn pidls(paths: &[PathBuf]) -> Pidls {
    let mut v = Vec::new();
    for p in paths {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        let h = HSTRING::from(p.as_os_str());
        if unsafe { SHParseDisplayName(PCWSTR(h.as_ptr()), None, &mut pidl, 0, None) }.is_ok() && !pidl.is_null() {
            v.push(pidl);
        }
    }
    Pidls(v)
}

/// Start an Explorer-style drag of these files (blocks until dropped or cancelled).
/// Returns true when something accepted the drop.
pub fn drag_files(paths: &[PathBuf]) -> bool {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let ids = pidls(paths);
    if ids.0.is_empty() {
        return false;
    }
    let consts: Vec<*const ITEMIDLIST> = ids.0.iter().map(|p| *p as *const _).collect();
    let r: windows::core::Result<DROPEFFECT> = (|| unsafe {
        let arr: IShellItemArray = SHCreateShellItemArrayFromIDLists(&consts)?;
        let data: IDataObject = arr.BindToHandler(None, &BHID_DataObject)?;
        SHDoDragDrop(None, &data, None, DROPEFFECT_COPY | DROPEFFECT_LINK)
    })();
    matches!(r, Ok(e) if e.0 != 0)
}

/// Open Explorer at the folder with all these files selected (they should share a folder).
pub fn reveal(paths: &[PathBuf]) {
    let Some(first) = paths.first() else { return };
    let Some(dir) = first.parent() else { return };
    let same: Vec<PathBuf> = paths.iter().filter(|p| p.parent() == Some(dir)).cloned().collect();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let folder = pidls(&[dir.to_path_buf()]);
    let items = pidls(&same);
    let ok = !folder.0.is_empty() && !items.0.is_empty() && {
        let consts: Vec<*const ITEMIDLIST> = items.0.iter().map(|p| *p as *const _).collect();
        unsafe { SHOpenFolderAndSelectItems(folder.0[0], Some(&consts), 0) }.is_ok()
    };
    if !ok {
        // explorer wants /select,"path" as is (Rust's own quoting of the whole
        // argument makes it open the default folder instead)
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", first.display())).spawn();
    }
}

pub fn open_file(p: &Path) {
    let _ = std::process::Command::new("explorer").arg(p).spawn();
}

/// Text file as UTF-8, falling back to the Korean ANSI code page (cp949).
pub fn read_text(p: &Path) -> Option<String> {
    let b = std::fs::read(p).ok()?;
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&b);
    if let Ok(s) = std::str::from_utf8(b) {
        return Some(s.to_string());
    }
    Some(cp949(b))
}

fn cp949(b: &[u8]) -> String {
    use windows::Win32::Globalization::{MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};
    unsafe {
        let n = MultiByteToWideChar(949, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), b, None);
        if n <= 0 {
            return String::from_utf8_lossy(b).into_owned();
        }
        let mut w = vec![0u16; n as usize];
        MultiByteToWideChar(949, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), b, Some(&mut w));
        String::from_utf16_lossy(&w)
    }
}

/// Is a mouse button physically held right now? (Either button: the user's
/// buttons may be swapped.) Used so a drag-out never starts from a fake event.
pub fn mouse_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 || (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0 }
}

/// Hold a named mutex for this root for the life of the process.
/// false = another imgr already holds it.
pub fn claim_instance(root: &str) -> bool {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::HSTRING;
    let h = crc32fast::hash(root.as_bytes());
    let name = HSTRING::from(format!("Local\\imgr_{h:08x}"));
    unsafe {
        match CreateMutexW(None, true, &name) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    return false;
                }
                // keep the handle open until the process ends
                let _ = handle;
                true
            }
            Err(_) => true,
        }
    }
}

fn focus_event(root: &str) -> HSTRING {
    let h = crc32fast::hash(root.as_bytes());
    HSTRING::from(format!("Local\\imgr_focus_{h:08x}"))
}

/// The imgr of this root listens here: a second start of imgr on the same
/// root rings this event (`request_focus`) instead of opening a window.
/// No network: a named Windows event only this user's programs can open.
pub fn on_focus_request(root: &str, f: impl Fn() + Send + 'static) {
    use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    let Ok(ev) = (unsafe { CreateEventW(None, false, false, &focus_event(root)) }) else { return };
    let raw = ev.0 as isize;
    let _ = std::thread::Builder::new().name("focus".into()).spawn(move || {
        let ev = HANDLE(raw as *mut _);
        while unsafe { WaitForSingleObject(ev, INFINITE) } == WAIT_OBJECT_0 {
            f();
        }
    });
}

/// Ask the imgr already running on this root to come to the front.
/// false = it has no such event (an older imgr): the caller may try another way.
pub fn request_focus(root: &str) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    unsafe {
        // let the other process take the foreground (we were started by the user)
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        match OpenEventW(EVENT_MODIFY_STATE, false, &focus_event(root)) {
            Ok(ev) => {
                let _ = SetEvent(ev);
                let _ = CloseHandle(ev);
                true
            }
            Err(_) => false,
        }
    }
}

/// Is this screen point on any connected monitor?
pub fn point_on_screen(x: f32, y: f32) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
    unsafe { !MonitorFromPoint(POINT { x: x as i32, y: y as i32 }, MONITOR_DEFAULTTONULL).is_invalid() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn instance_lock() {
        assert!(super::claim_instance("imgr-test-root-xyz"));
        assert!(!super::claim_instance("imgr-test-root-xyz"));
        assert!(super::point_on_screen(10.0, 10.0));
        assert!(!super::point_on_screen(-30000.0, -30000.0));
    }
}
