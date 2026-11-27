//! Win32 foreground metadata and native Edit password detection.

use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, LPARAM},
    System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    },
    UI::Controls::EM_GETPASSWORDCHAR,
    UI::WindowsAndMessaging::{
        ES_PASSWORD, EnumWindows, GUITHREADINFO, GWL_STYLE, GetClassNameW, GetForegroundWindow,
        GetGUIThreadInfo, GetWindowLongW, GetWindowThreadProcessId, IsWindowVisible,
        SMTO_ABORTIFHUNG, SendMessageTimeoutW,
    },
};

use crate::insert::Preflight;

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

pub(crate) fn foreground_process() -> Option<u32> {
    let mut pid = 0;
    // SAFETY: the API writes the PID into a live local.
    let thread = unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    (thread != 0).then_some(pid)
}

pub(crate) fn foreground_executable() -> (Option<String>, Option<u32>) {
    let pid = foreground_process();
    (pid.and_then(executable_for), pid)
}

pub fn frontmost_app_name() -> Option<String> {
    foreground_executable().0
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
pub(crate) fn preflight() -> Preflight {
    // SAFETY: Win32 fills a sized local struct and the fixed class-name buffer.
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if thread == 0 || GetGUIThreadInfo(thread, &mut info) == 0 || info.hwndFocus.is_null() {
            return Preflight::Paste;
        }
        let mut class = [0u16; 32];
        let len = GetClassNameW(info.hwndFocus, class.as_mut_ptr(), class.len() as i32) as usize;
        super::windows_field_copy_reason(&String::from_utf16_lossy(&class[..len]), || {
            let style = GetWindowLongW(info.hwndFocus, GWL_STYLE) as u32;
            if style & ES_PASSWORD as u32 != 0 {
                return true;
            }
            let mut password_char = 0;
            SendMessageTimeoutW(
                info.hwndFocus,
                EM_GETPASSWORDCHAR,
                0,
                0,
                SMTO_ABORTIFHUNG,
                100,
                &mut password_char,
            ) != 0
                && password_char != 0
        })
        .map_or(Preflight::Paste, Preflight::CopyOnly)
    }
}
