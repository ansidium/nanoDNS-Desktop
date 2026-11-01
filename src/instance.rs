//! Profile ownership and native, ordered activation with a separate reply per caller.
use crate::platform::MainWindow;
use eframe::egui;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    ffi::c_void,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ALREADY_EXISTS, GetLastError, HWND, SetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::Threading::{
        CreateEventW, EVENT_MODIFY_STATE, OpenEventW, OpenProcess, PROCESS_SYNCHRONIZE, SetEvent,
        WaitForMultipleObjects,
    },
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, GetWindowThreadProcessId, MSG, PostMessageW,
        RegisterWindowMessageW,
    },
};

const ENDPOINT: &str = "instance.json";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const ACTIVATION_WAIT_MS: u128 = 200;
const BUSY: &str = "nanoDNS уже запущен для этой папки настроек.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    pid: u32,
    event: String,
    window: isize,
    message: u32,
}

impl Endpoint {
    fn valid(&self) -> bool {
        let prefix = format!("Local\\nanoDNS.Desktop.{}.", self.pid);
        self.pid != 0
            && self.pid != u32::MAX
            && self.window != 0
            && (0xc000..=0xffff).contains(&self.message)
            && self.event.strip_prefix(&prefix).is_some_and(|suffix| {
                suffix.len() == 32 && suffix.bytes().all(|c| c.is_ascii_hexdigit())
            })
    }

    fn read(path: &Path) -> io::Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?.take(1025).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Endpoint too large",
            ));
        }
        let value: Self = serde_json::from_slice(&bytes)?;
        if !value.valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid endpoint",
            ));
        }
        Ok(value)
    }

    fn activate(&self, wait_ms: u32) -> io::Result<bool> {
        let window = self.window as HWND;
        let mut pid = 0_u32;
        // SAFETY: HWND is an opaque token validated by Windows, not dereferenced.
        // Windows writes only this live u32; destroyed handles return thread 0.
        let native_thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if native_thread == 0 || pid != self.pid {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Owner window expired",
            ));
        }
        // SAFETY: Windows validates pid. SYNCHRONIZE permits waiting for owner
        // termination only and gives no mutation rights; no handle is inherited.
        let owner = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, self.pid) };
        if owner.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcess returned a fresh valid owned handle, transferred
        // once and closed on every return path.
        let owner = unsafe { OwnedHandle::from_raw_handle(owner) };
        let nonce = positive_nonce()?;
        let client_pid = std::process::id();
        let reply = create_event(Some(&reply_name(&self.event, client_pid, nonce)))?;
        let client_parameter = usize::try_from(client_pid).map_err(io::Error::other)?;
        // SAFETY: Only the validated owner's PID receives focus permission.
        // PostMessageW copies integer parameters and does not retain Rust memory.
        // A separate registered message enters the ordinary native queue, behind
        // an already-posted tray Quit; no callback can bypass that older action.
        unsafe {
            let _ = AllowSetForegroundWindow(self.pid);
            if PostMessageW(window, self.message, client_parameter, nonce) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let handles = [owner.as_raw_handle(), reply.as_raw_handle()];
        // SAFETY: Distinct process/event handles remain owned through this wait
        // and have synchronization rights. Owner death has priority over a reply.
        let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, wait_ms) };
        match result {
            value if value == WAIT_OBJECT_0 + 1 => Ok(true),
            WAIT_OBJECT_0 | WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

pub enum Launch {
    Primary(Instance),
    Activated,
}

pub struct Reply {
    event: OwnedHandle,
}

#[derive(Clone)]
struct Target {
    window: MainWindow,
    ctx: egui::Context,
}

struct Shared {
    target: Mutex<Option<Target>>,
    closing: AtomicBool,
}

#[derive(Clone)]
pub struct Hook {
    message: u32,
    base: String,
    shared: Arc<Shared>,
    sender: SyncSender<Reply>,
}

impl Hook {
    /// Defer tray restoration until the outer message loop, after any native
    /// visibility commands in the current eframe output batch have completed.
    pub fn wake(&self, window: MainWindow) -> Result<(), String> {
        if self.message == 0 {
            return Err("Обработчик окна nanoDNS ещё не готов.".into());
        }
        // SAFETY: This non-owning HWND belongs to our app; Windows validates it.
        // The registered message carries only zero integers, never a pointer.
        if unsafe { PostMessageW(window.native_handle() as HWND, self.message, 0, 0) } == 0 {
            return Err(format!(
                "Не удалось открыть окно nanoDNS: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    /// # Safety
    /// `raw` must be null or point to the initialized, aligned Win32 MSG supplied
    /// by winit's Windows message hook, valid for this synchronous call only.
    pub unsafe fn on_message(&self, raw: *const c_void) -> bool {
        if raw.is_null() || self.message == 0 {
            return false;
        }
        // SAFETY: The caller supplies winit's live, aligned MSG pointer as
        // required above. Its data is borrowed only for this call, never stored.
        let message = unsafe { &*raw.cast::<MSG>() };
        if message.message != self.message {
            return false;
        }
        let target = match self.shared.target.lock() {
            Ok(target) => target.clone(),
            Err(_) => return false,
        };
        let Some(target) = target else {
            return false;
        };
        if message.hwnd != target.window.native_handle() as HWND {
            return false;
        }
        if self.shared.closing.load(Ordering::Acquire) {
            return true;
        }
        if message.wParam == 0 && message.lParam == 0 {
            // A tray callback has already queued its UI action. Restoring here
            // cannot be overwritten by that callback's surrounding output batch.
            let _ = target.window.restore();
            target.ctx.request_repaint();
            return true;
        }
        let Ok(client_pid) = u32::try_from(message.wParam) else {
            return true;
        };
        if client_pid == 0 || client_pid == u32::MAX || message.lParam <= 0 {
            return true;
        }
        let Ok(event) = open_event(
            &reply_name(&self.base, client_pid, message.lParam),
            EVENT_MODIFY_STATE,
        ) else {
            // A timed-out caller already closed its private reply event.
            return true;
        };
        if self.sender.try_send(Reply { event }).is_ok() {
            // Enqueue before native restore, including activation during startup.
            // The bounded queue never blocks the native loop. Only the UI may
            // acknowledge after its existing Quit/close actions are processed.
            let _ = target.window.restore();
            target.ctx.request_repaint();
        }
        true
    }
}

pub struct Instance {
    path: PathBuf,
    hook: Hook,
    pending: Receiver<Reply>,
    // Released last, after endpoint cleanup and the caller's DNS service.
    _lock: File,
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = self.begin_shutdown();
    }
}

impl Instance {
    fn new(lock: File, dir: &Path, headless: bool) -> Result<Self, String> {
        let path = dir.join(ENDPOINT);
        remove_endpoint(&path).map_err(|e| format!("Не удалось обновить данные запуска: {e}"))?;
        let (sender, pending) = mpsc::sync_channel(32);
        let mut hook = Hook {
            message: 0,
            base: String::new(),
            shared: Arc::new(Shared {
                target: Mutex::new(None),
                closing: AtomicBool::new(false),
            }),
            sender,
        };
        if !headless {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            hook.base = format!("Local\\nanoDNS.Desktop.{}.{nonce:032x}", std::process::id());
            // Registered IDs live until the Windows session ends. Reuse one
            // application message instead of consuming an ID on every relaunch;
            // HWND, owner PID and private reply names still isolate instances.
            let name = wide("nanoDNS.Desktop.Activate.v1");
            // SAFETY: name is a live terminated UTF-16 string. Windows returns a
            // session-wide registered message ID, retaining no Rust pointer.
            hook.message = unsafe { RegisterWindowMessageW(name.as_ptr()) };
            if !(0xc000..=0xffff).contains(&hook.message) {
                return Err(format!(
                    "Не удалось зарегистрировать открытие окна: {}",
                    io::Error::last_os_error()
                ));
            }
        }
        Ok(Self {
            path,
            hook,
            pending,
            _lock: lock,
        })
    }

    pub fn hook(&self) -> Hook {
        self.hook.clone()
    }

    pub fn attach(&mut self, window: MainWindow, ctx: egui::Context) -> Result<(), String> {
        if self.hook.message == 0 || self.hook.shared.closing.load(Ordering::Acquire) {
            return Err("Окно nanoDNS недоступно для повторного запуска.".into());
        }
        let endpoint = Endpoint {
            pid: std::process::id(),
            event: self.hook.base.clone(),
            window: window.native_handle(),
            message: self.hook.message,
        };
        let mut pid = 0_u32;
        // SAFETY: Windows validates the opaque HWND and writes this live u32.
        let native_thread = unsafe { GetWindowThreadProcessId(endpoint.window as HWND, &mut pid) };
        if native_thread == 0 || pid != endpoint.pid {
            return Err("Не удалось подключить окно nanoDNS.".into());
        }
        {
            let mut target = self
                .hook
                .shared
                .target
                .lock()
                .map_err(|_| "Не удалось подключить обработчик окна.")?;
            if target.is_some() {
                return Err("Обработчик окна уже подключён.".into());
            }
            *target = Some(Target { window, ctx });
        }
        let dir = self.path.parent().ok_or("Не указана папка настроек.")?;
        let mut pending = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut pending, &endpoint).map_err(|e| e.to_string())?;
        pending.flush().map_err(|e| e.to_string())?;
        pending.persist(&self.path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn try_activation(&self) -> Option<Reply> {
        self.pending.try_recv().ok()
    }

    /// Called only by the UI after Show is accepted and pending Quit is ruled out.
    pub fn acknowledge(&self, reply: Reply) -> Result<(), String> {
        if self.hook.shared.closing.load(Ordering::Acquire) {
            return Err("nanoDNS завершает работу.".into());
        }
        signal(&reply.event).map_err(|e| e.to_string())
    }

    /// Unpublish before the native window closes; keep ownership until DNS drops.
    pub fn begin_shutdown(&mut self) -> Result<(), String> {
        self.hook.shared.closing.store(true, Ordering::Release);
        let endpoint_result = remove_endpoint(&self.path);
        while self.pending.try_recv().is_ok() {}
        let target_result = self
            .hook
            .shared
            .target
            .lock()
            .map(|mut target| *target = None);
        endpoint_result.map_err(|e| format!("Не удалось удалить данные запуска: {e}"))?;
        target_result.map_err(|_| "Не удалось отключить обработчик окна.".into())
    }
}

pub fn acquire(dir: &Path, headless: bool, tray_start: bool) -> Result<Launch, String> {
    acquire_with_timeout(dir, headless, tray_start, STARTUP_TIMEOUT)
}

fn acquire_with_timeout(
    dir: &Path,
    headless: bool,
    tray_start: bool,
    timeout: Duration,
) -> Result<Launch, String> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("app.lock"))
        .map_err(|e| format!("Не удалось открыть блокировку настроек: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => return Instance::new(lock, dir, headless).map(Launch::Primary),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {}
            Err(error) => return Err(format!("Не удалось заблокировать настройки: {error}")),
        }
        if headless {
            return Err(BUSY.into());
        }
        if tray_start {
            return Ok(Launch::Activated);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait_ms = u32::try_from(remaining.as_millis().min(ACTIVATION_WAIT_MS))
            .map_err(|e| e.to_string())?;
        if let Ok(endpoint) = Endpoint::read(&dir.join(ENDPOINT))
            && endpoint.activate(wait_ms).is_ok_and(|accepted| accepted)
        {
            return Ok(Launch::Activated);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{BUSY}\nРаботающий экземпляр не подтвердил открытие окна. Если это предыдущая версия, выберите «Выйти» в её трее и запустите новую версию."
            ));
        }
        // Only the transient caller waits. No resident thread, timer or socket.
        thread::sleep(Duration::from_millis(40));
    }
}

fn remove_endpoint(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

fn positive_nonce() -> io::Result<isize> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let limit = u128::try_from(isize::MAX).map_err(io::Error::other)?;
    isize::try_from((time & limit).max(1)).map_err(io::Error::other)
}

fn reply_name(base: &str, pid: u32, nonce: isize) -> String {
    format!("{base}.reply.{pid}.{nonce:016x}")
}

fn open_event(name: &str, rights: u32) -> io::Result<OwnedHandle> {
    let name = wide(name);
    // SAFETY: name is a live terminated UTF-16 buffer. Windows validates access
    // and returns a non-inheritable owned event handle below.
    let handle = unsafe { OpenEventW(rights, 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Transfer unique ownership of the fresh valid handle exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn create_event(name: Option<&str>) -> io::Result<OwnedHandle> {
    let name = name.map(wide);
    // SAFETY: Optional name is terminated and lives for this synchronous call.
    // NULL security uses creator's default DACL, without inheritance. Each caller
    // owns a separate auto-reset, unsignaled event; collisions are rejected.
    let handle = unsafe {
        SetLastError(0);
        CreateEventW(
            std::ptr::null(),
            0,
            0,
            name.as_ref().map_or(std::ptr::null(), |name| name.as_ptr()),
        )
    };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Capture thread-local error immediately after CreateEventW.
    let existed = name.is_some() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // SAFETY: Transfer the newly returned non-null handle exactly once.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    if existed {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Reply event already exists",
        ));
    }
    Ok(handle)
}

fn signal(event: &OwnedHandle) -> io::Result<()> {
    // SAFETY: Borrowed valid event handle remains owned for this synchronous call.
    if unsafe { SetEvent(event.as_raw_handle()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    fn primary(dir: &Path) -> Instance {
        match acquire(dir, false, false).expect("acquire test instance") {
            Launch::Primary(instance) => instance,
            Launch::Activated => panic!("unexpected duplicate"),
        }
    }

    #[test]
    fn endpoint_is_not_published_before_window_attachment() {
        let dir = tempfile::tempdir().unwrap();
        let _owner = primary(dir.path());
        assert!(!dir.path().join(ENDPOINT).exists());
        let start = Instant::now();
        assert!(
            acquire_with_timeout(dir.path(), false, false, Duration::from_millis(100)).is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn background_duplicate_is_quiet_and_headless_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let owner = primary(dir.path());
        assert!(matches!(
            acquire(dir.path(), false, true).unwrap(),
            Launch::Activated
        ));
        assert!(owner.try_activation().is_none());
        assert!(acquire(dir.path(), true, false).is_err());
    }

    #[test]
    fn profiles_are_independent() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let _owner_a = primary(a.path());
        let _owner_b = primary(b.path());
        assert!(acquire(a.path(), true, false).is_err());
        assert!(acquire(b.path(), true, false).is_err());
    }

    #[test]
    fn shutdown_unpublishes_and_keeps_lock_until_drop() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner = primary(dir.path());
        fs::write(dir.path().join(ENDPOINT), "owned endpoint").unwrap();
        owner.begin_shutdown().unwrap();
        owner.begin_shutdown().unwrap();
        assert!(!dir.path().join(ENDPOINT).exists());
        assert!(acquire(dir.path(), true, false).is_err());
        drop(owner);
        let _next = primary(dir.path());
    }

    #[test]
    fn relaunch_during_shutdown_takes_ownership_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner = primary(dir.path());
        let profile = dir.path().to_owned();
        let duplicate = thread::spawn(move || {
            acquire_with_timeout(&profile, false, false, Duration::from_secs(2))
        });
        thread::sleep(Duration::from_millis(30));
        owner.begin_shutdown().unwrap();
        drop(owner);
        assert!(matches!(
            duplicate.join().unwrap().unwrap(),
            Launch::Primary(_)
        ));
    }

    #[test]
    fn stale_endpoint_is_removed_even_by_headless_owner() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ENDPOINT), "stale data").unwrap();
        let _owner = acquire(dir.path(), true, false).unwrap();
        assert!(!dir.path().join(ENDPOINT).exists());
    }

    #[test]
    fn invalid_or_oversized_endpoint_cannot_address_arbitrary_window_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ENDPOINT);
        let mut endpoint = Endpoint {
            pid: 12,
            event: "Local\\nanoDNS.Desktop.12.00000000000000000000000000000000".into(),
            window: 1,
            message: 0xc001,
        };
        fs::write(&path, serde_json::to_vec(&endpoint).unwrap()).unwrap();
        assert!(Endpoint::read(&path).is_ok());
        endpoint.message = 0x401;
        assert!(!endpoint.valid());
        endpoint.message = 0xc001;
        endpoint.window = 0;
        assert!(!endpoint.valid());
        endpoint.window = 1;
        endpoint.pid = 13;
        assert!(!endpoint.valid());
        for invalid in [b"{not json}".to_vec(), vec![b' '; 2048]] {
            fs::write(&path, invalid).unwrap();
            assert!(Endpoint::read(&path).is_err());
        }
    }

    #[test]
    fn reply_ack_is_per_client_and_shutdown_rejects_ack() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner = primary(dir.path());
        let name_a = reply_name(&owner.hook.base, std::process::id(), 1);
        let name_b = reply_name(&owner.hook.base, std::process::id(), 2);
        let client_a = create_event(Some(&name_a)).unwrap();
        let client_b = create_event(Some(&name_b)).unwrap();
        owner
            .acknowledge(Reply {
                event: open_event(&name_a, EVENT_MODIFY_STATE).unwrap(),
            })
            .unwrap();
        // SAFETY: Both handles are live owned event handles; zero timeout is a
        // nonblocking state probe and neither is closed while being inspected.
        unsafe {
            assert_eq!(
                WaitForSingleObject(client_a.as_raw_handle(), 0),
                WAIT_OBJECT_0
            );
            assert_eq!(
                WaitForSingleObject(client_b.as_raw_handle(), 0),
                WAIT_TIMEOUT
            );
        }
        owner.begin_shutdown().unwrap();
        assert!(
            owner
                .acknowledge(Reply {
                    event: open_event(&name_b, EVENT_MODIFY_STATE).unwrap()
                })
                .is_err()
        );
    }

    #[test]
    fn hook_rejects_null_and_unnamed_event_ignores_previous_last_error() {
        let dir = tempfile::tempdir().unwrap();
        let owner = primary(dir.path());
        // SAFETY: Null is explicitly allowed by Hook's contract. SetLastError
        // changes only the Windows error slot of this test thread.
        unsafe {
            assert!(!owner.hook().on_message(std::ptr::null()));
            SetLastError(ERROR_ALREADY_EXISTS);
        }
        let _event = create_event(None).unwrap();
    }
}
