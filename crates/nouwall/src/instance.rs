//! Single instance and messages from `nouwallctl` / a second `nouwall` launch
//! to the running engine (WM_COPYDATA to the hidden controller window).

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COPYDATA};

pub const CONTROLLER_CLASS: windows::core::PCWSTR = w!("nouwall-controller");
pub const COPYDATA_TAG: usize = 0x4E57_0001;

pub struct InstanceLock(HANDLE);

impl Drop for InstanceLock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// `Some(lock)` when this is the only engine instance for the user session.
pub fn acquire() -> Option<InstanceLock> {
    unsafe {
        let h = CreateMutexW(None, true, w!("Local\\nouwall-engine")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            None
        } else {
            Some(InstanceLock(h))
        }
    }
}

pub fn is_running() -> bool {
    unsafe { FindWindowW(CONTROLLER_CLASS, None).is_ok() }
}

/// Send a command (`reload`, `settings`, `pause`, `resume`, `toggle`, `quit`) to the engine.
pub fn send(command: &str) -> bool {
    unsafe {
        let Ok(hwnd) = FindWindowW(CONTROLLER_CLASS, None) else { return false };
        let data = COPYDATASTRUCT {
            dwData: COPYDATA_TAG,
            cbData: command.len() as u32,
            lpData: command.as_ptr() as *mut _,
        };
        let mut result = 0usize;
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&data as *const _ as isize),
            SMTO_ABORTIFHUNG,
            5000,
            Some(&mut result),
        )
        .0 != 0
    }
}
