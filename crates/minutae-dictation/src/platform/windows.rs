//! Win32 foreground metadata and paste into verified standard edit controls.

use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, LPARAM},
    System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    },
    UI::Controls::EM_GETPASSWORDCHAR,
    UI::WindowsAndMessaging::{
        ES_PASSWORD, ES_READONLY, EnumWindows, GUITHREADINFO, GWL_STYLE, GetClassNameW,
        GetForegroundWindow, GetGUIThreadInfo, GetWindowLongW, GetWindowThreadProcessId,
        IsWindowVisible, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_PASTE,
    },
};

use crate::insert::{CopyReason, Preflight};

fn executable_for(pid: u32) -> Option<String> {
    // SAFETY: every handle opened here is closed, and the buffer is sized in UTF-16 units.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    }
}

pub fn foreground_executable() -> Option<(String, u32)> {
    // SAFETY: the API writes the PID into a live local.
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    Some((executable_for(pid)?, pid))
}

pub fn frontmost_app_name() -> Option<String> {
    foreground_executable().map(|(name, _)| name)
}

pub(crate) fn visible_executables() -> Vec<String> {
    unsafe extern "system" fn collect(window: HWND, data: LPARAM) -> i32 {
        let mut pid = 0;
        // SAFETY: EnumWindows invokes the callback synchronously with the same live Vec pointer.
        unsafe {
            if IsWindowVisible(window) != 0 {
                GetWindowThreadProcessId(window, &mut pid);
                if pid != std::process::id()
                    && let Some(name) = executable_for(pid)
                {
                    (&mut *(data as *mut Vec<String>)).push(name);
                }
            }
        }
        1
    }
    let mut names: Vec<String> = Vec::new();
    // SAFETY: the callback uses the pointer only within EnumWindows.
    unsafe { EnumWindows(Some(collect), (&mut names as *mut Vec<String>) as LPARAM) };
    names
}

/// Native Edit controls expose ES_PASSWORD even when their value is empty.
/// Custom, browser, and unknown controls fall back to the clipboard.
fn focused_edit(process: u32) -> Result<HWND, CopyReason> {
    // SAFETY: Win32 fills a sized local struct and the fixed class-name buffer.
    unsafe {
        let window = GetForegroundWindow();
        let mut pid = 0;
        let thread = GetWindowThreadProcessId(window, &mut pid);
        if thread == 0 || pid != process {
            return Err(CopyReason::FocusChanged);
        }
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread, &mut info) == 0 || info.hwndFocus.is_null() {
            return Err(CopyReason::UnverifiedField);
        }
        let mut focused_pid = 0;
        GetWindowThreadProcessId(info.hwndFocus, &mut focused_pid);
        if focused_pid != process || GetForegroundWindow() != window {
            return Err(CopyReason::FocusChanged);
        }
        let mut class = [0u16; 32];
        let len = GetClassNameW(info.hwndFocus, class.as_mut_ptr(), class.len() as i32) as usize;
        if !String::from_utf16_lossy(&class[..len]).eq_ignore_ascii_case("Edit") {
            return Err(CopyReason::UnverifiedField);
        }
        let style = GetWindowLongW(info.hwndFocus, GWL_STYLE) as u32;
        if style == 0 || style & ES_READONLY as u32 != 0 {
            return Err(CopyReason::NoTextField);
        }
        if style & ES_PASSWORD as u32 != 0 {
            return Err(CopyReason::SecureInput);
        }
        let mut password_char = 0;
        if SendMessageTimeoutW(
            info.hwndFocus,
            EM_GETPASSWORDCHAR,
            0,
            0,
            SMTO_ABORTIFHUNG,
            100,
            &mut password_char,
        ) == 0
        {
            return Err(CopyReason::UnverifiedField);
        }
        if password_char != 0 {
            return Err(CopyReason::SecureInput);
        }
        let checked = info.hwndFocus;
        if GetForegroundWindow() != window
            || GetGUIThreadInfo(thread, &mut info) == 0
            || info.hwndFocus != checked
        {
            return Err(CopyReason::FocusChanged);
        }
        Ok(info.hwndFocus)
    }
}

pub(crate) fn preflight(process: u32) -> Preflight {
    match focused_edit(process) {
        Ok(_) => Preflight::Paste,
        Err(reason) => Preflight::CopyOnly(reason),
    }
}

pub(crate) fn paste(process: u32) -> Result<(), CopyReason> {
    let target = focused_edit(process)?;
    // SAFETY: WM_PASTE carries no pointers and is sent only to the verified control.
    let sent = unsafe {
        SendMessageTimeoutW(
            target,
            WM_PASTE,
            0,
            0,
            SMTO_ABORTIFHUNG,
            250,
            std::ptr::null_mut(),
        )
    };
    if sent == 0 {
        Err(CopyReason::PasteFailed)
    } else {
        Ok(())
    }
}
