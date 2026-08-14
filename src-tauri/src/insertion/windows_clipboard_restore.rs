use std::ptr;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use windows_sys::Win32::Foundation::{
    GetLastError, GlobalFree, SetLastError, ERROR_SUCCESS, HANDLE,
};
use windows_sys::Win32::Graphics::Gdi::{
    CopyEnhMetaFileW, DeleteEnhMetaFile, DeleteMetaFile, DeleteObject, HGDIOBJ,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, SetClipboardData, METAFILEPICT,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::{
    OleDuplicateData, CF_BITMAP, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT,
    CF_ENHMETAFILE, CF_GDIOBJFIRST, CF_GDIOBJLAST, CF_METAFILEPICT, CF_OWNERDISPLAY, CF_PALETTE,
};

const OPEN_ATTEMPTS: usize = 8;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(5);

/// Eager duplicates of all concrete formats currently offered by the Windows
/// clipboard. `OleDuplicateData` handles normal global-memory formats as well
/// as bitmap, palette, and metafile handles.
pub(super) struct ClipboardSnapshot {
    entries: Vec<ClipboardEntry>,
}

struct ClipboardEntry {
    format: u32,
    handle: HANDLE,
}

impl ClipboardSnapshot {
    pub(super) fn capture() -> Result<Self> {
        let _clipboard = OpenClipboardGuard::open(ptr::null_mut())?;
        let mut snapshot = Self {
            entries: Vec::new(),
        };
        let mut previous_format = 0;

        loop {
            unsafe { SetLastError(ERROR_SUCCESS) };
            let format = unsafe { EnumClipboardFormats(previous_format) };
            if format == 0 {
                let error = unsafe { GetLastError() };
                if error == ERROR_SUCCESS {
                    break;
                }
                return Err(anyhow!(
                    "could not enumerate Windows clipboard formats: OS error {error}"
                ));
            }
            previous_format = format;

            // Owner-display content is painted by the source window and has
            // no transferable data handle. Every concrete format is copied.
            if format == u32::from(CF_OWNERDISPLAY) {
                continue;
            }

            let source = unsafe { GetClipboardData(format) };
            if source.is_null() {
                return Err(anyhow!(
                    "could not materialize Windows clipboard format {format}: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let duplicate = unsafe { duplicate_handle(format, source) };
            if duplicate.is_null() {
                return Err(anyhow!(
                    "could not duplicate Windows clipboard format {format}: {}",
                    std::io::Error::last_os_error()
                ));
            }

            snapshot.entries.push(ClipboardEntry {
                format,
                handle: duplicate,
            });
        }

        Ok(snapshot)
    }

    pub(super) fn restore_if_unchanged(
        mut self,
        expected_sequence_number: u32,
        owner: windows_sys::Win32::Foundation::HWND,
    ) -> Result<bool> {
        let _clipboard = OpenClipboardGuard::open(owner)?;
        if !should_restore(expected_sequence_number, unsafe {
            GetClipboardSequenceNumber()
        }) {
            return Ok(false);
        }

        if unsafe { EmptyClipboard() } == 0 {
            return Err(anyhow!(
                "could not clear the temporary dictation clipboard: {}",
                std::io::Error::last_os_error()
            ));
        }

        for entry in &mut self.entries {
            if unsafe { SetClipboardData(entry.format, entry.handle) }.is_null() {
                return Err(anyhow!(
                    "could not restore Windows clipboard format {}: {}",
                    entry.format,
                    std::io::Error::last_os_error()
                ));
            }

            // SetClipboardData transfers ownership to the operating system.
            entry.handle = ptr::null_mut();
        }

        Ok(true)
    }
}

impl Drop for ClipboardSnapshot {
    fn drop(&mut self) {
        for entry in &mut self.entries {
            if !entry.handle.is_null() {
                unsafe { free_duplicated_handle(entry.format, entry.handle) };
                entry.handle = ptr::null_mut();
            }
        }
    }
}

struct OpenClipboardGuard;

impl OpenClipboardGuard {
    fn open(owner: windows_sys::Win32::Foundation::HWND) -> Result<Self> {
        for attempt in 0..OPEN_ATTEMPTS {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Self);
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                thread::sleep(OPEN_RETRY_DELAY);
            }
        }

        Err(anyhow!(
            "could not open the Windows clipboard: {}",
            std::io::Error::last_os_error()
        ))
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

pub(super) fn sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

fn should_restore(expected_sequence_number: u32, current_sequence_number: u32) -> bool {
    expected_sequence_number == current_sequence_number
}

unsafe fn duplicate_handle(format: u32, source: HANDLE) -> HANDLE {
    if format == u32::from(CF_ENHMETAFILE) || format == u32::from(CF_DSPENHMETAFILE) {
        // OleDuplicateData predates enhanced metafiles. Copy this GDI handle
        // explicitly so it remains valid after the clipboard is replaced.
        unsafe { CopyEnhMetaFileW(source, ptr::null()) }
    } else {
        unsafe { OleDuplicateData(source, format as u16, 0) }
    }
}

unsafe fn free_duplicated_handle(format: u32, handle: HANDLE) {
    match format {
        value
            if value == u32::from(CF_BITMAP)
                || value == u32::from(CF_DSPBITMAP)
                || value == u32::from(CF_PALETTE)
                || (u32::from(CF_GDIOBJFIRST)..=u32::from(CF_GDIOBJLAST)).contains(&value) =>
        unsafe {
            DeleteObject(handle as HGDIOBJ);
        },
        value if value == u32::from(CF_ENHMETAFILE) || value == u32::from(CF_DSPENHMETAFILE) => unsafe {
            DeleteEnhMetaFile(handle);
        },
        value if value == u32::from(CF_METAFILEPICT) || value == u32::from(CF_DSPMETAFILEPICT) => unsafe {
            let picture = GlobalLock(handle) as *const METAFILEPICT;
            if !picture.is_null() {
                if !(*picture).hMF.is_null() {
                    DeleteMetaFile((*picture).hMF);
                }
                GlobalUnlock(handle);
            }
            GlobalFree(handle);
        },
        _ => unsafe {
            GlobalFree(handle);
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_only_when_blabber_still_owns_the_latest_clipboard_change() {
        assert!(should_restore(42, 42));
        assert!(!should_restore(42, 43));
    }
}
