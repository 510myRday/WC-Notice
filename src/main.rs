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
        Box::new(move |_cc| Ok(Box::new(WcNoticeApp::new(Arc::clone(&engine), schedule)))),
    )
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
