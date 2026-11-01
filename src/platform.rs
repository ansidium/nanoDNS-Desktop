use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    ffi::OsStr,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};
use windows_sys::Win32::{
    Foundation::{DWM_E_COMPOSITIONDISABLED, E_INVALIDARG, HWND},
    Graphics::{
        Dwm::{
            DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmGetWindowAttribute,
            DwmSetWindowAttribute,
        },
        Gdi::InvalidateRect,
    },
    System::Threading::GetCurrentThreadId,
    UI::{
        Shell::ShellExecuteW,
        WindowsAndMessaging::{
            GetWindowPlacement, GetWindowThreadProcessId, IsIconic, IsWindowVisible, MB_ICONERROR,
            MB_OK, MessageBoxW, SPI_GETCLIENTAREAANIMATION, SW_HIDE, SW_SHOW, SW_SHOWMAXIMIZED,
            SW_SHOWNORMAL, SetForegroundWindow, ShowWindow, ShowWindowAsync, SystemParametersInfoW,
            WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
        },
    },
};
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE},
};

use base64::Engine;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_NAME: &str = "NanoDNS Desktop";

pub fn lifecycle_trace(message: std::fmt::Arguments<'_>) {
    if std::env::var_os("NANODNS_LIFECYCLE_TRACE").is_some() {
        eprintln!("nanoDNS[{}] {message}", std::process::id());
    }
}

/// Read the client-area animation preference, independently of window minimize effects.
pub fn client_animations_enabled() -> bool {
    let mut enabled = 1_i32; // Win32 BOOL is a 32-bit signed integer.
    // SAFETY: SPI_GETCLIENTAREAANIMATION reads system state into this live,
    // initialized, aligned BOOL. No pointer is retained and no setting changes.
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            std::ptr::from_mut(&mut enabled).cast(),
            0,
        )
    };
    // An unavailable query must not silently turn off the requested animation.
    result == 0 || enabled != 0
}

#[derive(Clone, Copy)]
pub struct MainWindow {
    // A non-owning opaque Windows handle, represented as an integer so callbacks
    // may pass it between threads without claiming Rust pointer ownership.
    handle: isize,
}

impl MainWindow {
    /// Non-owning opaque HWND for the same-process native message hook.
    pub fn native_handle(self) -> isize {
        self.handle
    }

    pub fn from_context(context: &eframe::CreationContext<'_>) -> Result<Self, String> {
        match context
            .window_handle()
            .map_err(|e| {
                format!("Не удалось получить окно nanoDNS. Сворачивание в трей недоступно: {e}")
            })?
            .as_raw()
        {
            RawWindowHandle::Win32(handle) => {
                let window = Self {
                    handle: handle.hwnd.get(),
                };
                // Cosmetic opt-in only: Windows 10 does not support this attribute,
                // and DWM can legitimately decline rounding in VM/snapped windows.
                // Neither case should disable the window or its tray controls.
                let _ = window.request_rounded_corners();
                Ok(window)
            }
            _ => Err(
                "Windows не предоставила дескриптор окна. Сворачивание в трей недоступно.".into(),
            ),
        }
    }

    fn checked_handle(self) -> Result<HWND, String> {
        let handle = self.handle as HWND;
        let mut process = 0_u32;
        // SAFETY: HWND is an opaque OS token, never dereferenced by Rust. Windows
        // validates it and writes only to this live u32; stale handles return 0.
        let thread = unsafe { GetWindowThreadProcessId(handle, &mut process) };
        if thread == 0 || process != std::process::id() {
            return Err("Окно nanoDNS больше недоступно.".into());
        }
        Ok(handle)
    }

    fn request_rounded_corners(self) -> Result<(), String> {
        let handle = self.checked_handle()?;
        let preference = DWMWCP_ROUND;
        let attribute = u32::try_from(DWMWA_WINDOW_CORNER_PREFERENCE)
            .map_err(|_| "Некорректный атрибут оформления окна.".to_string())?;
        let size = u32::try_from(std::mem::size_of_val(&preference))
            .map_err(|_| "Некорректный размер атрибута оформления окна.".to_string())?;
        // SAFETY: This is our checked, non-owning HWND. The attribute expects a
        // DWM_WINDOW_CORNER_PREFERENCE (i32); its initialized value and exact byte
        // count remain valid for the synchronous call. DWM retains no Rust pointer.
        let result = unsafe {
            DwmSetWindowAttribute(
                handle,
                attribute,
                std::ptr::from_ref(&preference).cast(),
                size,
            )
        };
        if result < 0 {
            return Err(format!(
                "DWM не применил скругление окна: HRESULT {result:#010x}."
            ));
        }
        Ok(())
    }

    /// Reports the compositor's stored preference, not the visible corner pixels.
    /// Windows keeps maximized/snapped/virtualized windows square by design.
    pub fn rounded_corner_preference(self) -> Result<Option<bool>, String> {
        let handle = self.checked_handle()?;
        let mut preference = 0_i32;
        let attribute = u32::try_from(DWMWA_WINDOW_CORNER_PREFERENCE)
            .map_err(|_| "Некорректный атрибут оформления окна.".to_string())?;
        let size = u32::try_from(std::mem::size_of_val(&preference))
            .map_err(|_| "Некорректный размер атрибута оформления окна.".to_string())?;
        // SAFETY: Same checked HWND and typed attribute as the setter. DWM can
        // write at most the supplied size into this live, aligned i32 and does
        // not retain the exclusive pointer after returning.
        let result = unsafe {
            DwmGetWindowAttribute(
                handle,
                attribute,
                std::ptr::from_mut(&mut preference).cast(),
                size,
            )
        };
        if matches!(result, E_INVALIDARG | DWM_E_COMPOSITIONDISABLED) {
            return Ok(None);
        }
        if result < 0 {
            return Err(format!(
                "Не удалось проверить скругление окна: HRESULT {result:#010x}."
            ));
        }
        Ok(Some(preference == DWMWCP_ROUND))
    }

    pub fn restore(self) -> Result<(), String> {
        let handle = self.checked_handle()?;
        // SAFETY: The handle was captured from our eframe window and checked
        // against this process. These OS calls validate even a concurrently
        // closed HWND; they borrow it and never transfer or free ownership.
        unsafe {
            // Choose explicit, idempotent show commands. Repeated SW_RESTORE
            // calls can unmaximize a window restored by an earlier callback.
            let command = if IsIconic(handle) != 0 {
                let mut placement = WINDOWPLACEMENT {
                    length: u32::try_from(std::mem::size_of::<WINDOWPLACEMENT>())
                        .map_err(|e| e.to_string())?,
                    ..Default::default()
                };
                // placement is initialized, aligned and exactly length bytes;
                // GetWindowPlacement writes synchronously and retains no pointer.
                if GetWindowPlacement(handle, &mut placement) == 0 {
                    return Err("Не удалось прочитать состояние окна nanoDNS.".into());
                }
                if placement.flags & WPF_RESTORETOMAXIMIZED != 0 {
                    SW_SHOWMAXIMIZED
                } else {
                    SW_SHOWNORMAL
                }
            } else {
                SW_SHOW
            };
            let window_thread = GetWindowThreadProcessId(handle, std::ptr::null_mut());
            if window_thread == GetCurrentThreadId() {
                // Restore before eframe synchronizes its minimized state.
                // Zero means previously hidden, not failure, for ShowWindow.
                let _ = ShowWindow(handle, command);
            } else if ShowWindowAsync(handle, command) == 0 {
                return Err("Windows не смогла восстановить окно nanoDNS.".into());
            }
            // Foreground activation can be denied by Windows. Visibility still
            // restores, and the next UI pass applies its normal focus request.
            let _ = SetForegroundWindow(handle);
            // egui can coalesce a repaint request with an earlier hidden frame
            // which Windows never delivered. Invalidate the client area itself
            // so restoration receives a fresh WM_PAINT even in that state.
            // No UPDATENOW/SendMessage: painting must not re-enter a UI borrow.
            if InvalidateRect(handle, std::ptr::null(), 0) == 0 {
                return Err("Windows не смогла обновить окно nanoDNS.".into());
            }
        }
        Ok(())
    }

    pub fn minimized(self) -> Result<bool, String> {
        let handle = self.checked_handle()?;
        // SAFETY: Windows checks this borrowed HWND; no pointer is dereferenced.
        Ok(unsafe { IsIconic(handle) } != 0)
    }

    pub fn visible(self) -> Result<bool, String> {
        let handle = self.checked_handle()?;
        // SAFETY: Same checked, non-owning HWND as restore; no Rust memory is read.
        Ok(unsafe { IsWindowVisible(handle) } != 0)
    }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

pub fn error_dialog(text: &str) {
    let title = wide(OsStr::new("nanoDNS Desktop"));
    let message = wide(OsStr::new(text));
    // SAFETY: Both buffers are terminated UTF-16 and live for the whole synchronous call; HWND is null.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn data_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("NanoDNS Desktop"))
        .ok_or("Windows не предоставила папку LOCALAPPDATA.".into())
}

fn startup_command(executable: &Path) -> Result<String, String> {
    let text = executable
        .to_str()
        .ok_or("Путь приложения содержит некорректный Unicode.")?;
    if text.contains('"') {
        return Err("Некорректный путь приложения.".into());
    }
    Ok(format!("\"{text}\" --tray"))
}

pub fn startup_enabled() -> Result<bool, String> {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let key = match root.open_subkey_with_flags(RUN_KEY, KEY_READ) {
        Ok(key) => key,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    match key.get_value::<String, _>(RUN_NAME) {
        Ok(value) => {
            Ok(value == startup_command(&std::env::current_exe().map_err(|e| e.to_string())?)?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

pub fn set_startup(enabled: bool) -> Result<(), String> {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = root
        .create_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
        .map_err(|e| format!("Нет доступа к автозапуску: {e}"))?;
    if enabled {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value(RUN_NAME, &startup_command(&executable)?)
            .map_err(|e| e.to_string())
    } else {
        match key.delete_value(RUN_NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub fn open_folder(path: &Path) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn open_url(url: &str) -> Result<(), String> {
    let verb = wide(OsStr::new("open"));
    let target = wide(OsStr::new(url));
    // SAFETY: Valid terminated UTF-16 strings; no owner window, parameters or working directory.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if (result as isize) <= 32 {
        Err("Не удалось открыть браузер.".into())
    } else {
        Ok(())
    }
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn firewall_script(exe: &str, result: &str, port: u16) -> String {
    let pending = format!("{result}.pending");
    format!(
        "$ErrorActionPreference = 'Stop'\r\ntry {{\r\n  $existing = Get-NetFirewallRule -Name 'NanoDNS.Desktop.UDP' -ErrorAction SilentlyContinue\r\n  if ($existing) {{ $existing | Remove-NetFirewallRule }}\r\n  New-NetFirewallRule -Name 'NanoDNS.Desktop.UDP' -DisplayName 'nanoDNS Desktop - local DNS' -Direction Inbound -Action Allow -Protocol UDP -LocalPort {port} -Program {} -Profile Private,Public -RemoteAddress LocalSubnet | Out-Null\r\n  $message = 'OK'\r\n}} catch {{\r\n  $message = $_.Exception.Message\r\n}}\r\nSet-Content -LiteralPath {} -Value $message -Encoding UTF8\r\nMove-Item -LiteralPath {} -Destination {} -Force\r\n",
        ps_quote(exe),
        ps_quote(&pending),
        ps_quote(&pending),
        ps_quote(result),
    )
}

pub fn allow_firewall(data_dir: &Path, port: u16) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let result_path = data_dir.join("firewall-result.txt");
    let exe = executable.to_str().ok_or("Некорректный путь приложения.")?;
    let result = result_path.to_str().ok_or("Некорректный путь журнала.")?;
    let script = firewall_script(exe, result, port);
    // Encode the complete command so no editable script is executed with elevated rights.
    // PowerShell's EncodedCommand uses UTF-16LE, preserving Cyrillic paths.
    let mut encoded = Vec::new();
    for unit in script.encode_utf16() {
        encoded.extend_from_slice(&unit.to_le_bytes());
    }
    if result_path.exists() {
        std::fs::remove_file(&result_path).map_err(|e| e.to_string())?;
    }
    let args = format!(
        "-NoProfile -WindowStyle Hidden -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(encoded)
    );
    let verb = wide(OsStr::new("runas"));
    let executable = wide(OsStr::new("powershell.exe"));
    let args = wide(OsStr::new(&args));
    // SAFETY: Terminated UTF-16 buffers live through ShellExecuteW. `runas` explicitly asks the user for UAC approval.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            executable.as_ptr(),
            args.as_ptr(),
            std::ptr::null(),
            SW_HIDE,
        )
    };
    if (result as isize) <= 32 {
        Err("Доступ не изменён: запрос Windows отменён или не удалось открыть настройки.".into())
    } else {
        Ok(())
    }
}

pub fn firewall_configured(port: u16) -> Result<bool, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = executable.to_str().ok_or("Некорректный путь приложения.")?;
    let script = format!(
        "$ErrorActionPreference='Stop'; $r=Get-NetFirewallRule -Name 'NanoDNS.Desktop.UDP' -ErrorAction SilentlyContinue; if ($r) {{ $p=$r|Get-NetFirewallPortFilter; $a=$r|Get-NetFirewallApplicationFilter; $s=$r|Get-NetFirewallAddressFilter; if ($r.Enabled -eq 'True' -and $r.Direction -eq 'Inbound' -and $r.Action -eq 'Allow' -and [int]$r.Profile -eq 6 -and $p.Protocol -eq 'UDP' -and $p.LocalPort -eq '{port}' -and $a.Program -eq {} -and $s.RemoteAddress -contains 'LocalSubnet') {{ Write-Output 'OK' }} }}; exit 0",
        ps_quote(exe)
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Не удалось проверить брандмауэр Windows.".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "OK")
}

pub fn open_network_settings() -> Result<(), String> {
    Command::new("explorer.exe")
        .arg("ms-settings:network-status")
        .creation_flags(0x0800_0000)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_path_with_spaces_and_unicode() {
        assert_eq!(
            startup_command(Path::new("C:\\Программы\\nano DNS.exe")).unwrap(),
            "\"C:\\Программы\\nano DNS.exe\" --tray"
        );
        assert_eq!(ps_quote("C:\\O'Brien\\file"), "'C:\\O''Brien\\file'");
    }
    #[test]
    fn firewall_script_executes_with_mocked_cmdlets_and_preserves_paths() {
        let temp = tempfile::tempdir().unwrap();
        let result = temp.path().join("result.txt");
        let plan = temp.path().join("plan.json");
        let executable = "C:\\Программы\\O'Brien\\nanoDNS.exe";
        let mocks = format!(
            "function Get-NetFirewallRule {{ param($Name,$ErrorAction) }}; function Remove-NetFirewallRule {{ throw 'unexpected removal' }}; function New-NetFirewallRule {{ param($Name,$DisplayName,$Direction,$Action,$Protocol,$LocalPort,$Program,$Profile,$RemoteAddress); [pscustomobject]@{{Name=$Name;Program=$Program;Profile=$Profile;RemoteAddress=$RemoteAddress;LocalPort=$LocalPort;Protocol=$Protocol}} | ConvertTo-Json | Set-Content -LiteralPath {} -Encoding UTF8 }}; ",
            ps_quote(plan.to_str().unwrap())
        );
        let script = mocks + &firewall_script(executable, result.to_str().unwrap(), 53);
        let encoded = script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &base64::engine::general_purpose::STANDARD.encode(encoded),
            ])
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(output.status.success(), "PowerShell firewall script syntax");
        assert_eq!(
            std::fs::read_to_string(result)
                .unwrap()
                .trim_start_matches('\u{feff}')
                .trim(),
            "OK"
        );
        let text = std::fs::read_to_string(plan).unwrap();
        let plan: serde_json::Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        assert_eq!(plan["Program"], executable);
        assert_eq!(plan["Name"], "NanoDNS.Desktop.UDP");
        assert_eq!(plan["Protocol"], "UDP");
        assert_eq!(plan["RemoteAddress"], "LocalSubnet");
        assert_eq!(plan["LocalPort"], 53);
        assert_eq!(plan["Profile"], serde_json::json!(["Private", "Public"]));
    }

    #[test]
    fn firewall_completion_is_not_visible_during_a_partial_write() {
        let temp = tempfile::tempdir().unwrap();
        let result = temp.path().join("result.txt");
        // Model a slow writer deterministically. The UI polls only `result`,
        // which must stay absent until both parts have been written.
        let mocks = "function Get-NetFirewallRule { param($Name,$ErrorAction) }; function New-NetFirewallRule { param($Name,$DisplayName,$Direction,$Action,$Protocol,$LocalPort,$Program,$Profile,$RemoteAddress) }; function Set-Content { param($LiteralPath,$Value,$Encoding); [IO.File]::WriteAllText($LiteralPath, ''); Start-Sleep -Milliseconds 300; [IO.File]::WriteAllText($LiteralPath, $Value) }; ";
        let script =
            mocks.to_owned() + &firewall_script("C:\\nanoDNS.exe", result.to_str().unwrap(), 53);
        let encoded = script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &base64::engine::general_purpose::STANDARD.encode(encoded),
            ])
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut partial = false;
        let status = loop {
            if let Ok(contents) = std::fs::read_to_string(&result) {
                partial |= contents.trim() != "OK";
            }
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Mocked firewall command timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(status.success(), "Mocked firewall command failed");
        assert!(!partial, "UI observed an incomplete firewall result");
        assert_eq!(std::fs::read_to_string(&result).unwrap().trim(), "OK");
        assert!(!result.with_extension("txt.pending").exists());
    }
    #[test]
    fn firewall_detection_runs_read_only() {
        assert!(!firewall_configured(65530).unwrap());
    }
}
