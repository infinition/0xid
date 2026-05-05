#[derive(Debug)]
pub enum TrayEvent {
    Show,
    Quit,
}

// ══════════════════════════════════════════════════════════════════════════════
// Windows implementation
// ══════════════════════════════════════════════════════════════════════════════
#[cfg(windows)]
mod platform {
    use super::TrayEvent;
    use std::sync::{
        atomic::{AtomicIsize, Ordering},
        mpsc, Mutex,
    };

    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::Shell::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const WM_TRAYICON: u32 = WM_USER + 1;
    const HOTKEY_ID: i32 = 1;

    static TRAY_SENDER: Mutex<Option<mpsc::Sender<TrayEvent>>> = Mutex::new(None);
    static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
    /// The egui application window HWND — set on first frame
    static APP_HWND: AtomicIsize = AtomicIsize::new(0);
    static EGUI_CTX: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);

    pub struct TrayManager {
        rx: mpsc::Receiver<TrayEvent>,
    }

    impl TrayManager {
        pub fn new() -> Self {
            let (tx, rx) = mpsc::channel();
            *TRAY_SENDER.lock().unwrap() = Some(tx);

            std::thread::spawn(|| unsafe { tray_thread() });

            // Give the tray thread a moment to set up
            std::thread::sleep(std::time::Duration::from_millis(100));

            TrayManager { rx }
        }

        pub fn poll(&self) -> Option<TrayEvent> {
            self.rx.try_recv().ok()
        }

        /// Store the egui app window HWND so we can show/hide it
        pub fn set_app_hwnd(&self, hwnd: isize) {
            APP_HWND.store(hwnd, Ordering::SeqCst);
        }

        /// Give the tray thread access to the egui Context for wake-up
        pub fn set_egui_ctx(&self, ctx: eframe::egui::Context) {
            *EGUI_CTX.lock().unwrap() = Some(ctx);
        }

        /// Hide the egui window via Win32 ShowWindow
        pub fn hide(&self) {
            let hwnd = APP_HWND.load(Ordering::SeqCst);
            if hwnd != 0 {
                unsafe {
                    ShowWindow(hwnd, SW_HIDE);
                }
            }
        }

        /// Show the egui window via Win32 ShowWindow
        #[allow(dead_code)]
        pub fn show(&self) {
            let hwnd = APP_HWND.load(Ordering::SeqCst);
            if hwnd != 0 {
                unsafe {
                    ShowWindow(hwnd, SW_SHOW);
                    SetForegroundWindow(hwnd);
                }
            }
        }
    }

    impl Drop for TrayManager {
        fn drop(&mut self) {
            unsafe {
                let hwnd = TRAY_HWND.load(Ordering::SeqCst);
                if hwnd != 0 {
                    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                    nid.hWnd = hwnd;
                    nid.uID = 1;
                    Shell_NotifyIconW(NIM_DELETE, &nid);
                    UnregisterHotKey(hwnd, HOTKEY_ID);
                }
            }
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_tip(s: &str) -> [u16; 128] {
        let mut buf = [0u16; 128];
        for (i, c) in s.encode_utf16().take(127).enumerate() {
            buf[i] = c;
        }
        buf
    }

    const STARTUP_REG_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";
    const STARTUP_VALUE_NAME: &str = "0xID";

    fn is_startup_enabled() -> bool {
        use windows_sys::Win32::System::Registry::*;

        unsafe {
            let key_path = wide(STARTUP_REG_KEY);
            let value_name = wide(STARTUP_VALUE_NAME);
            let mut hkey: isize = 0;

            let res = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                0,
                KEY_READ,
                &mut hkey,
            );
            if res != 0 {
                return false;
            }

            let res = RegQueryValueExW(
                hkey,
                value_name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            RegCloseKey(hkey);
            res == 0
        }
    }

    fn toggle_startup() {
        use windows_sys::Win32::System::Registry::*;

        if is_startup_enabled() {
            // Remove from startup
            unsafe {
                let key_path = wide(STARTUP_REG_KEY);
                let value_name = wide(STARTUP_VALUE_NAME);
                let mut hkey: isize = 0;

                let res = RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    key_path.as_ptr(),
                    0,
                    KEY_WRITE,
                    &mut hkey,
                );
                if res == 0 {
                    RegDeleteValueW(hkey, value_name.as_ptr());
                    RegCloseKey(hkey);
                }
            }
        } else {
            // Add to startup (hidden: starts minimized to tray)
            let exe_path = std::env::current_exe().unwrap_or_default();
            let cmd = format!("\"{}\" --hidden", exe_path.to_string_lossy());

            unsafe {
                let key_path = wide(STARTUP_REG_KEY);
                let value_name = wide(STARTUP_VALUE_NAME);
                let value_data: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
                let mut hkey: isize = 0;

                let res = RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    key_path.as_ptr(),
                    0,
                    KEY_WRITE,
                    &mut hkey,
                );
                if res == 0 {
                    RegSetValueExW(
                        hkey,
                        value_name.as_ptr(),
                        0,
                        REG_SZ,
                        value_data.as_ptr() as *const u8,
                        (value_data.len() * 2) as u32,
                    );
                    RegCloseKey(hkey);
                }
            }
        }
    }

    fn send_event(event: TrayEvent) {
        if let Ok(guard) = TRAY_SENDER.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(event);
            }
        }
        // Wake up egui event loop
        if let Ok(guard) = EGUI_CTX.lock() {
            if let Some(ctx) = guard.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    /// Directly show the app window from the tray thread
    fn show_app_window() {
        let hwnd = APP_HWND.load(Ordering::SeqCst);
        if hwnd != 0 {
            unsafe {
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
            }
        }
    }

    // ── Tray thread ─────────────────────────────────────────────────────────

    unsafe fn tray_thread() {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name = wide("OxidTrayClass");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(tray_proc),
            hInstance: hinstance,
            lpszClassName: class_name.as_ptr(),
            style: 0,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hIcon: 0,
            hCursor: 0,
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            hIconSm: 0,
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            -3isize, // HWND_MESSAGE
            0,
            hinstance,
            std::ptr::null(),
        );
        TRAY_HWND.store(hwnd, Ordering::SeqCst);

        // Add tray icon
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        // Embed the .ico at compile time and write to a temp file for LoadImageW
        let ico_bytes = include_bytes!("../assets/0xid.ico");
        let tmp_ico = std::env::temp_dir().join("0xid_tray.ico");
        let _ = std::fs::write(&tmp_ico, ico_bytes);
        let wide_ico = wide(&tmp_ico.to_string_lossy());
        let mut icon = LoadImageW(
            0,
            wide_ico.as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        ) as isize;
        if icon == 0 {
            icon = LoadIconW(0, IDI_APPLICATION);
        }
        nid.hIcon = icon;
        nid.szTip = wide_tip("0xID \u{2014} Ctrl+Shift+Space");
        Shell_NotifyIconW(NIM_ADD, &nid);

        // Global hotkey: Ctrl + Shift + Space
        RegisterHotKey(hwnd, HOTKEY_ID, MOD_CONTROL | MOD_SHIFT, VK_SPACE as u32);

        // Message pump
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        Shell_NotifyIconW(NIM_DELETE, &nid);
    }

    unsafe extern "system" fn tray_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_HOTKEY if wparam == HOTKEY_ID as usize => {
                // Ctrl+Shift+Space: show window immediately + send event
                show_app_window();
                send_event(TrayEvent::Show);
                0
            }
            x if x == WM_TRAYICON => {
                let mouse_msg = lparam as u32;
                match mouse_msg {
                    WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                        show_app_window();
                        send_event(TrayEvent::Show);
                    }
                    WM_RBUTTONUP => {
                        let menu = CreatePopupMenu();
                        let show_text = wide("Show 0xID");
                        let startup_text = if is_startup_enabled() {
                            wide("\u{2713} Start with Windows")
                        } else {
                            wide("Start with Windows")
                        };
                        let quit_text = wide("Quit");
                        AppendMenuW(menu, 0, 1, show_text.as_ptr());
                        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                        AppendMenuW(menu, 0, 3, startup_text.as_ptr());
                        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                        AppendMenuW(menu, 0, 2, quit_text.as_ptr());

                        let mut pt: POINT = std::mem::zeroed();
                        GetCursorPos(&mut pt);
                        SetForegroundWindow(hwnd);
                        let cmd = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_NONOTIFY,
                            pt.x,
                            pt.y,
                            0,
                            hwnd,
                            std::ptr::null(),
                        );
                        DestroyMenu(menu);

                        match cmd {
                            1 => {
                                show_app_window();
                                send_event(TrayEvent::Show);
                            }
                            2 => {
                                send_event(TrayEvent::Quit);
                                let mut nid_del: NOTIFYICONDATAW = std::mem::zeroed();
                                nid_del.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                                nid_del.hWnd = hwnd;
                                nid_del.uID = 1;
                                Shell_NotifyIconW(NIM_DELETE, &nid_del);
                                std::process::exit(0);
                            }
                            3 => {
                                toggle_startup();
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Non-Windows stub
// ══════════════════════════════════════════════════════════════════════════════
#[cfg(not(windows))]
mod platform {
    use super::TrayEvent;

    pub struct TrayManager;

    impl TrayManager {
        pub fn new() -> Self {
            TrayManager
        }
        pub fn poll(&self) -> Option<TrayEvent> {
            None
        }
        pub fn set_app_hwnd(&self, _hwnd: isize) {}
        pub fn set_egui_ctx(&self, _ctx: eframe::egui::Context) {}
        pub fn hide(&self) {}
        pub fn show(&self) {}
    }
}

pub use platform::TrayManager;
