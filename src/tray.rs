use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use eframe::egui;

#[derive(Default)]
struct TraySignals {
    show_requested: AtomicBool,
    exit_requested: AtomicBool,
}

impl TraySignals {
    fn request_show(&self) {
        self.show_requested.store(true, Ordering::Release);
    }

    fn request_exit(&self) {
        self.exit_requested.store(true, Ordering::Release);
    }

    fn take_show_request(&self) -> bool {
        self.show_requested.swap(false, Ordering::AcqRel)
    }

    fn take_exit_request(&self) -> bool {
        self.exit_requested.swap(false, Ordering::AcqRel)
    }
}

/// 主线程持有的托盘句柄。
///
/// 只包含 `Arc` 包裹的共享状态，均实现了 `Send + Sync`，
/// 可安全地从托盘线程传回主线程。
/// 实际的 `TrayIcon`（内含 `Rc`，非 `Send`）留在托盘线程中。
pub struct TrayHandle {
    signals: Arc<TraySignals>,
    repaint_ctx: Arc<Mutex<Option<egui::Context>>>,
}

impl TrayHandle {
    /// 创建共享信号对，返回 `(TrayHandle, TrayThreadState)`。
    ///
    /// - `TrayHandle`：传给主线程，用于查询托盘事件信号。
    /// - `TrayThreadState`：在托盘线程中调用 [`TrayThreadState::run`] 完成托盘初始化并运行消息泵。
    ///
    /// `init_tx` 用于在托盘初始化完成后立即通知主线程（成功/失败），
    /// 通知发出后消息泵继续在托盘线程中运行，主线程不再阻塞。
    pub fn new_split(
        icon_bytes: &'static [u8],
        init_tx: std::sync::mpsc::SyncSender<bool>,
    ) -> (TrayHandle, TrayThreadState) {
        let signals = Arc::new(TraySignals::default());
        let repaint_ctx = Arc::new(Mutex::new(None::<egui::Context>));

        let handle = TrayHandle {
            signals: Arc::clone(&signals),
            repaint_ctx: Arc::clone(&repaint_ctx),
        };

        let state = TrayThreadState {
            icon_bytes,
            signals,
            repaint_ctx,
            init_tx,
        };

        (handle, state)
    }

    pub fn bind_egui_ctx(&self, ctx: &egui::Context) {
        if let Ok(mut slot) = self.repaint_ctx.lock() {
            *slot = Some(ctx.clone());
        }
    }

    pub fn take_show_request(&self) -> bool {
        self.signals.take_show_request()
    }

    pub fn take_exit_request(&self) -> bool {
        self.signals.take_exit_request()
    }
}

/// 托盘线程状态，持有初始化托盘所需的全部数据。
///
/// 此结构体是 `Send`（`Arc` 字段均为 `Send + Sync`，`&'static [u8]` 也是 `Send`），
/// 可安全地移入 `std::thread::spawn` 闭包。
pub struct TrayThreadState {
    icon_bytes: &'static [u8],
    signals: Arc<TraySignals>,
    repaint_ctx: Arc<Mutex<Option<egui::Context>>>,
    /// 初始化完成后立即通过此 channel 通知主线程，然后继续运行消息泵。
    init_tx: std::sync::mpsc::SyncSender<bool>,
}

impl TrayThreadState {
    /// 在托盘线程中调用：
    /// 1. 初始化托盘图标
    /// 2. 通过 `init_tx` 立即通知主线程初始化结果（不等消息泵退出）
    /// 3. 若初始化成功，运行 Win32 消息泵直到退出
    pub fn run(self) {
        #[cfg(target_os = "windows")]
        {
            let init_ok = self.init_tray_windows();
            // ★ 关键：初始化完成后立即通知主线程，不等消息泵退出
            let _ = self.init_tx.send(init_ok);
            if init_ok {
                self.run_message_pump_windows();
            }
        }

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let init_ok = self.init_tray_unix();
            let _ = self.init_tx.send(init_ok);
            if init_ok {
                self.run_message_pump_unix();
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            log::warn!("当前平台暂不支持系统托盘");
            let _ = self.init_tx.send(false);
        }
    }

    #[cfg(target_os = "windows")]
    fn init_tray_windows(&self) -> bool {
        use anyhow::Context as _;
        use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
        use tray_icon::{
            Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent, TrayIconId,
        };

        const SHOW_MENU_ID: &str = "wc_notice.tray.show";
        const EXIT_MENU_ID: &str = "wc_notice.tray.exit";

        let result: anyhow::Result<()> = (|| {
            let image = image::load_from_memory(self.icon_bytes)
                .context("读取托盘图标失败")?
                .to_rgba8();
            let (width, height) = image.dimensions();
            let icon = Icon::from_rgba(image.into_raw(), width, height)
                .map_err(|e| anyhow::anyhow!("托盘图标解码失败: {e}"))?;

            let tray_menu = Menu::new();
            let show_id = MenuId::new(SHOW_MENU_ID);
            let exit_id = MenuId::new(EXIT_MENU_ID);
            let show_item = MenuItem::with_id(show_id.clone(), "显示主界面", true, None);
            let exit_item = MenuItem::with_id(exit_id.clone(), "退出", true, None);

            tray_menu
                .append_items(&[&show_item, &PredefinedMenuItem::separator(), &exit_item])
                .context("初始化托盘菜单失败")?;

            let signals_for_menu = Arc::clone(&self.signals);
            let repaint_ctx_for_menu = Arc::clone(&self.repaint_ctx);
            let show_id_for_menu = show_id.clone();
            let exit_id_for_menu = exit_id.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id == show_id_for_menu {
                    signals_for_menu.request_show();
                    wake_main_window(&repaint_ctx_for_menu);
                } else if event.id == exit_id_for_menu {
                    signals_for_menu.request_exit();
                    wake_main_window(&repaint_ctx_for_menu);
                }
            }));

            let tray_id = TrayIconId::new("wc_notice.tray.icon");
            let signals_for_click = Arc::clone(&self.signals);
            let repaint_ctx_for_click = Arc::clone(&self.repaint_ctx);
            let tray_id_for_click = tray_id.clone();
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if event.id() != &tray_id_for_click {
                    return;
                }

                let should_restore = matches!(
                    &event,
                    TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } | TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                );

                if should_restore {
                    signals_for_click.request_show();
                    wake_main_window(&repaint_ctx_for_click);
                }
            }));

            // 注意：_tray_icon 必须保持存活，否则托盘图标会消失。
            // 用 Box::leak 将其泄漏到 'static，确保在消息泵线程中永久存活。
            let tray_icon = TrayIconBuilder::new()
                .with_id(tray_id)
                .with_icon(icon)
                .with_tooltip("WC Notice")
                .with_menu(Box::new(tray_menu))
                .with_menu_on_left_click(false)
                .build()
                .context("创建托盘图标失败")?;

            Box::leak(Box::new(tray_icon));

            Ok(())
        })();

        match result {
            Ok(()) => {
                log::info!("托盘图标初始化成功");
                true
            }
            Err(e) => {
                log::warn!("托盘初始化失败，将不启用托盘功能: {e}");
                false
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn run_message_pump_windows(&self) {
        log::info!("托盘消息泵线程启动");
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                DispatchMessageW, GetMessageW, MSG, TranslateMessage,
            };
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        log::info!("托盘消息泵线程退出");
    }

    /// Linux / macOS 托盘初始化。
    /// tray-icon 在这两个平台上使用 GTK（Linux）或 NSStatusItem（macOS），
    /// 不需要独立的 Win32 消息泵，事件由 tray-icon 内部机制分发。
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn init_tray_unix(&self) -> bool {
        use anyhow::Context as _;
        use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
        use tray_icon::{
            Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent, TrayIconId,
        };

        const SHOW_MENU_ID: &str = "wc_notice.tray.show";
        const EXIT_MENU_ID: &str = "wc_notice.tray.exit";

        let result: anyhow::Result<()> = (|| {
            let image = image::load_from_memory(self.icon_bytes)
                .context("读取托盘图标失败")?
                .to_rgba8();
            let (width, height) = image.dimensions();
            let icon = Icon::from_rgba(image.into_raw(), width, height)
                .map_err(|e| anyhow::anyhow!("托盘图标解码失败: {e}"))?;

            let tray_menu = Menu::new();
            let show_id = MenuId::new(SHOW_MENU_ID);
            let exit_id = MenuId::new(EXIT_MENU_ID);
            let show_item = MenuItem::with_id(show_id.clone(), "显示主界面", true, None);
            let exit_item = MenuItem::with_id(exit_id.clone(), "退出", true, None);

            tray_menu
                .append_items(&[&show_item, &PredefinedMenuItem::separator(), &exit_item])
                .context("初始化托盘菜单失败")?;

            let signals_for_menu = Arc::clone(&self.signals);
            let repaint_ctx_for_menu = Arc::clone(&self.repaint_ctx);
            let show_id_for_menu = show_id.clone();
            let exit_id_for_menu = exit_id.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id == show_id_for_menu {
                    signals_for_menu.request_show();
                    wake_main_window(&repaint_ctx_for_menu);
                } else if event.id == exit_id_for_menu {
                    signals_for_menu.request_exit();
                    wake_main_window(&repaint_ctx_for_menu);
                }
            }));

            let tray_id = TrayIconId::new("wc_notice.tray.icon");
            let signals_for_click = Arc::clone(&self.signals);
            let repaint_ctx_for_click = Arc::clone(&self.repaint_ctx);
            let tray_id_for_click = tray_id.clone();
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if event.id() != &tray_id_for_click {
                    return;
                }
                let should_restore = matches!(
                    &event,
                    TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } | TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                );
                if should_restore {
                    signals_for_click.request_show();
                    wake_main_window(&repaint_ctx_for_click);
                }
            }));

            let tray_icon = TrayIconBuilder::new()
                .with_id(tray_id)
                .with_icon(icon)
                .with_tooltip("WC Notice")
                .with_menu(Box::new(tray_menu))
                .with_menu_on_left_click(false)
                .build()
                .context("创建托盘图标失败")?;

            Box::leak(Box::new(tray_icon));
            Ok(())
        })();

        match result {
            Ok(()) => {
                log::info!("托盘图标初始化成功");
                true
            }
            Err(e) => {
                log::warn!("托盘初始化失败，将不启用托盘功能: {e}");
                false
            }
        }
    }

    /// Linux / macOS 消息泵：tray-icon 在这两个平台上依赖主线程事件循环，
    /// 但由于 eframe 已经在主线程运行事件循环，托盘事件会通过 tray-icon 的
    /// 内部回调机制触发，不需要额外的消息泵循环。
    /// 此处用简单的 sleep 循环保持线程存活（托盘图标已 leak，不会被 drop）。
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn run_message_pump_unix(&self) {
        log::info!("托盘线程保活循环启动");
        // 托盘图标已通过 Box::leak 保持存活，此线程只需保持运行即可。
        // 实际事件分发由 tray-icon 内部机制处理。
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
}

fn wake_main_window(repaint_ctx: &Arc<Mutex<Option<egui::Context>>>) {
    if let Ok(slot) = repaint_ctx.lock() {
        if let Some(ctx) = slot.as_ref() {
            ctx.request_repaint();
        }
    }
}
