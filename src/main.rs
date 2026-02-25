#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod engine;
mod notifier;
mod schedule;
mod tray;

use std::sync::Arc;

use app::WcNoticeApp;
use engine::Engine;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("WC Notice 启动中...");

    // 加载应用配置
    let config = config::load_config();
    log::info!("已加载配置，时间表数量: {}", config.schedules.len());

    // 创建引擎并启动后台检测线程
    let engine = Arc::new(Engine::new(config.clone()));
    engine.start();

    // 在专用线程中创建托盘图标并运行 Win32 消息泵。
    // tray-icon 要求：托盘图标必须在与 Win32 消息泵相同的线程上创建。
    // eframe/winit 只泵送自己管理的窗口消息，不会泵送 tray-icon 隐藏 HWND 的消息，
    // 因此必须在独立线程中运行 GetMessage/DispatchMessage 循环。
    //
    // 方案：new_split() 返回 (TrayHandle, TrayThreadState)：
    //   - TrayHandle 只含 Arc 字段（Send），传回主线程使用
    //   - TrayThreadState 移入专用线程，完成托盘初始化并运行消息泵
    let mut tray = {
        // 使用 SyncSender（容量=1），托盘线程在初始化完成后立即发送结果，
        // 然后继续运行消息泵。主线程收到信号后即可继续启动 eframe，不再阻塞。
        let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<bool>(1);

        let (handle, thread_state) =
            tray::TrayHandle::new_split(include_bytes!("../assets/icon.ico"), init_tx);

        std::thread::Builder::new()
            .name("tray-msg-pump".to_string())
            .spawn(move || {
                // run() 内部：初始化托盘 → 立即通过 init_tx 通知主线程 → 运行消息泵
                thread_state.run();
            })
            .expect("无法创建托盘消息泵线程");

        // 等待托盘线程完成初始化（init_tx 在初始化后立即发送，不等消息泵退出）
        match init_rx.recv() {
            Ok(true) => {
                log::info!("托盘功能已启用");
                Some(handle)
            }
            Ok(false) => {
                log::warn!("托盘初始化失败，将不启用托盘功能");
                None
            }
            Err(_) => {
                log::warn!("托盘线程异常退出，将不启用托盘功能");
                None
            }
        }
    };

    // 启动 egui GUI
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("WC Notice - 作息提醒")
            .with_inner_size([780.0, 520.0])
            .with_min_inner_size([600.0, 400.0])
            .with_icon(load_app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "WC Notice",
        native_options,
        Box::new(move |cc| {
            // 加载中文字体，解决 Windows/macOS 中文乱码问题
            setup_chinese_font(&cc.egui_ctx);
            Ok(Box::new(WcNoticeApp::new(
                Arc::clone(&engine),
                config,
                tray.take(),
            )))
        }),
    )
}

/// 从系统字体路径加载中文字体并注册到 egui
///
/// 优先级：
///   Windows  → 微软雅黑 (msyh.ttc)
///   macOS    → 苹方 (PingFang.ttc) → 华文黑体 (STHeiti Medium.ttc)
///   Linux    → Noto Sans CJK SC → WenQuanYi Micro Hei
fn setup_chinese_font(ctx: &egui::Context) {
    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc", // 微软雅黑
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simsun.ttc", // 宋体 fallback
    ];

    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/System/Library/Fonts/PingFang.ttc",       // 苹方
        "/System/Library/Fonts/STHeiti Medium.ttc", // 华文黑体
        "/System/Library/Fonts/Supplemental/Arial Unicode MS.ttf",
    ];

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let candidates: &[&str] = &[
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJKsc-Regular.otf",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
    ];

    // 找到第一个可读的字体文件
    let font_data = candidates
        .iter()
        .find_map(|path| match std::fs::read(path) {
            Ok(data) => {
                log::info!("已加载系统中文字体: {}", path);
                Some(data)
            }
            Err(_) => None,
        });

    let Some(font_data) = font_data else {
        log::warn!("未找到系统中文字体，界面中文可能显示为方块");
        return;
    };

    // 将字体注册进 egui 字体系统
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "chinese_sys".to_owned(),
        egui::FontData::from_owned(font_data).into(),
    );

    // 将中文字体追加到 Proportional 和 Monospace 字族末尾
    // （egui 会按顺序 fallback，先用内置拉丁字体，找不到字形再用中文字体）
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("chinese_sys".to_owned());
    }

    ctx.set_fonts(fonts);
    log::info!("中文字体注册完成");
}

/// 加载应用图标（内嵌 ICO）
fn load_app_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.ico");
    match image::load_from_memory(icon_bytes) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            }
        }
        Err(e) => {
            log::warn!("图标加载失败，使用默认图标: {}", e);
            egui::IconData::default()
        }
    }
}
