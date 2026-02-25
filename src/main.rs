#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod engine;
mod notifier;
mod schedule;

use std::sync::Arc;

use app::WcNoticeApp;
use engine::Engine;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("WC Notice 启动中...");

    // 加载时间表配置
    let schedule = config::load_schedule();
    log::info!("已加载时间表: {}", schedule.name);

    // 创建引擎并启动后台检测线程
    let engine = Arc::new(Engine::new(schedule.clone()));
    engine.start();

    // 启动 egui GUI
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("🔔 WC Notice - 作息提醒")
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
            Ok(Box::new(WcNoticeApp::new(Arc::clone(&engine), schedule)))
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
        r"C:\Windows\Fonts\msyh.ttc",    // 微软雅黑
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simsun.ttc",  // 宋体 fallback
    ];

    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/System/Library/Fonts/PingFang.ttc",                        // 苹方
        "/System/Library/Fonts/STHeiti Medium.ttc",                  // 华文黑体
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
    let font_data = candidates.iter().find_map(|path| {
        match std::fs::read(path) {
            Ok(data) => {
                log::info!("已加载系统中文字体: {}", path);
                Some(data)
            }
            Err(_) => None,
        }
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

/// 加载应用图标（内嵌 PNG）
fn load_app_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.png");
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
