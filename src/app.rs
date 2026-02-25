use chrono::{Local, NaiveTime};
use eframe::egui;
use eframe::egui::{Align, Color32, FontFamily, FontId, RichText, Stroke, TextStyle, Ui};
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::save_config;
use crate::engine::Engine;
use crate::schedule;
use crate::schedule::{AppConfig, BuiltinSound, Period, PeriodKind, ScheduleProfile, SoundSource};
use crate::tray::TrayHandle;

const MIN_CONTENT_WIDTH: f32 = 720.0;
const PERIOD_ROW_MIN_HEIGHT: f32 = 38.0;
const PERIOD_TIME_WIDTH: f32 = 96.0;
const PERIOD_KIND_WIDTH: f32 = 80.0;
const PERIOD_NAME_MIN_WIDTH: f32 = 120.0;
const PERIOD_STATUS_WIDTH: f32 = 34.0;
const PERIOD_DELETE_WIDTH: f32 = 56.0;

pub struct WcNoticeApp {
    engine: Arc<Engine>,
    config: AppConfig,
    tray: Option<TrayHandle>,
    status_msg: String,
    theme_applied: bool,
    show_exit_confirm_dialog: bool,
    allow_window_close: bool,
    viewport_was_minimized: bool,
    /// 正在从托盘恢复中：跳过本帧及下一帧的最小化检测，
    /// 避免 restore 命令异步生效前被 handle_window_lifecycle 再次最小化。
    /// 值为剩余需要跳过的帧数（通常设为 2）。
    restoring_from_tray_frames: u8,
    /// 任务栏按钮是否已被隐藏（避免每帧重复调用 Win32 API）
    taskbar_hidden: bool,
    last_active_schedule_id: Option<u64>,

    // 新建时间表
    new_schedule_name: String,
    // 重命名当前时间表
    rename_schedule_name: String,

    // 新增节点表单
    new_period_time: String,
    new_period_name: String,
    new_period_kind: PeriodKind,

    // 弹窗控制
    show_schedule_window: bool,
    show_new_schedule_window: bool,
    show_sound_window: bool,
    show_add_dialog: bool,
    show_settings_window: bool,

    // 防抖：记录最后一次"脏"时刻，延迟写盘
    pending_save: Option<Instant>,
    pending_save_msg: String,
}

impl WcNoticeApp {
    pub fn new(engine: Arc<Engine>, mut config: AppConfig, tray: Option<TrayHandle>) -> Self {
        config.ensure_active_schedule();
        let active_id = config.active_schedule_id;
        let rename = config
            .active_schedule()
            .map(|schedule| schedule.name.clone())
            .unwrap_or_default();

        let app = Self {
            engine,
            config,
            tray,
            status_msg: "就绪".to_string(),
            theme_applied: false,
            show_exit_confirm_dialog: false,
            allow_window_close: false,
            viewport_was_minimized: false,
            restoring_from_tray_frames: 0,
            taskbar_hidden: false,
            last_active_schedule_id: active_id,
            new_schedule_name: String::new(),
            rename_schedule_name: rename,
            new_period_time: "00:00:00".to_string(),
            new_period_name: "新节点".to_string(),
            new_period_kind: PeriodKind::Start,
            show_schedule_window: false,
            show_new_schedule_window: false,
            show_sound_window: false,
            show_add_dialog: false,
            show_settings_window: false,
            pending_save: None,
            pending_save_msg: String::new(),
        };
        app.apply_autostart();
        app
    }

    /// 同步开机自启状态到系统注册表（仅 Windows）
    fn apply_autostart(&self) {
        #[cfg(target_os = "windows")]
        {
            use winreg::RegKey;
            use winreg::enums::*;
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let run_key = hkcu
                .open_subkey_with_flags(
                    r"Software\Microsoft\Windows\CurrentVersion\Run",
                    KEY_SET_VALUE,
                )
                .ok();
            if let Some(key) = run_key {
                if self.config.autostart {
                    if let Ok(exe_path) = std::env::current_exe() {
                        let _ = key.set_value("WcNotice", &exe_path.to_string_lossy().as_ref());
                    }
                } else {
                    let _ = key.delete_value("WcNotice");
                }
            }
        }
    }

    /// 标记数据已变更：立即同步到引擎，延迟 500ms 写盘（防抖）
    fn mark_dirty(&mut self, success_msg: impl Into<String>) {
        self.config.ensure_active_schedule();
        self.engine.update_config(self.config.clone());
        self.pending_save_msg = success_msg.into();
        self.pending_save = Some(Instant::now());
    }

    /// 在 update() 帧开头调用：到期则真正写盘
    fn flush_pending_save(&mut self) {
        if self
            .pending_save
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(500))
        {
            self.pending_save = None;
            let msg = std::mem::take(&mut self.pending_save_msg);
            match save_config(&self.config) {
                Ok(_) => self.status_msg = msg,
                Err(e) => self.status_msg = format!("保存失败: {e}"),
            }
        }
    }

    fn sync_rename_name_from_active(&mut self) {
        if self.last_active_schedule_id != self.config.active_schedule_id {
            self.rename_schedule_name = self
                .config
                .active_schedule()
                .map(|schedule| schedule.name.clone())
                .unwrap_or_default();
            self.last_active_schedule_id = self.config.active_schedule_id;
        }
    }

    fn active_schedule(&self) -> Option<&ScheduleProfile> {
        self.config.active_schedule()
    }

    fn active_schedule_mut(&mut self) -> Option<&mut ScheduleProfile> {
        self.config.active_schedule_mut()
    }

    fn handle_tray_events(&mut self, ctx: &egui::Context) {
        let mut show_requested = false;
        let mut exit_requested = false;

        if let Some(tray) = &self.tray {
            tray.bind_egui_ctx(ctx);
            show_requested = tray.take_show_request();
            exit_requested = tray.take_exit_request();
        }

        if show_requested {
            self.restore_from_tray(ctx);
        }

        if exit_requested {
            self.restore_from_tray(ctx);
            self.show_exit_confirm_dialog = true;
        }
    }

    fn minimize_to_tray(&mut self, ctx: &egui::Context) {
        if self.tray.is_none() {
            return;
        }

        // 使用 Minimized(true) 而非 Visible(false)：
        // Visible(false) 会让 eframe 停止渲染帧，update() 不再被调用，
        // 导致 handle_tray_events() 无法执行，托盘点击永远无响应。
        // Minimized(true) 保持 update() 继续运行，托盘事件可正常处理。
        // hide_taskbar_button() 在下一帧窗口确认最小化后再调用（见 handle_window_lifecycle）。
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        self.viewport_was_minimized = true;
        self.status_msg = "已最小化到托盘，点击托盘图标可恢复".to_string();
    }

    fn restore_from_tray(&mut self, ctx: &egui::Context) {
        // 先恢复任务栏按钮样式，再发送 viewport 命令
        self.show_taskbar_button();
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.viewport_was_minimized = false;
        // 设置跳过帧计数：viewport 命令异步生效，需跳过接下来 2 帧的最小化检测，
        // 防止 handle_window_lifecycle 在命令生效前再次触发最小化。
        self.restoring_from_tray_frames = 2;
    }

    /// 隐藏任务栏按钮：通过 Win32 API 找到应用窗口，
    /// 移除 WS_EX_APPWINDOW，添加 WS_EX_TOOLWINDOW，使其从任务栏消失。
    /// 使用 SetWindowPos+SWP_FRAMECHANGED 刷新样式，不调用 ShowWindow 以免停止 eframe 渲染循环。
    #[cfg(target_os = "windows")]
    fn hide_taskbar_button(&self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GWL_EXSTYLE, GetWindowLongPtrW, HWND_NOTOPMOST, SWP_FRAMECHANGED,
            SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_EX_APPWINDOW,
            WS_EX_TOOLWINDOW,
        };
        unsafe {
            // 窗口标题与 main.rs 中 with_title() 保持一致
            let title: Vec<u16> = "WC Notice - 作息提醒\0".encode_utf16().collect();
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if hwnd.is_null() {
                return;
            }
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            // 移除 WS_EX_APPWINDOW，添加 WS_EX_TOOLWINDOW
            let new_style = (ex_style & !(WS_EX_APPWINDOW as isize)) | (WS_EX_TOOLWINDOW as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
            // SWP_FRAMECHANGED 通知系统刷新扩展样式；HWND_NOTOPMOST 确保不置顶
            SetWindowPos(
                hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn hide_taskbar_button(&self) {}

    /// 恢复任务栏按钮：移除 WS_EX_TOOLWINDOW，添加 WS_EX_APPWINDOW，并将窗口带到前台。
    #[cfg(target_os = "windows")]
    fn show_taskbar_button(&self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GWL_EXSTYLE, GetWindowLongPtrW, HWND_NOTOPMOST, SWP_FRAMECHANGED,
            SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW,
            SetWindowPos, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        };
        unsafe {
            let title: Vec<u16> = "WC Notice - 作息提醒\0".encode_utf16().collect();
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if hwnd.is_null() {
                return;
            }
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let new_style = (ex_style & !(WS_EX_TOOLWINDOW as isize)) | (WS_EX_APPWINDOW as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
            // 刷新样式，确保不置顶
            SetWindowPos(
                hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
            // 强制将窗口带到前台
            SetForegroundWindow(hwnd);
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn show_taskbar_button(&self) {}

    fn handle_window_lifecycle(&mut self, ctx: &egui::Context) {
        if self.tray.is_some() {
            // 正在恢复中：跳过最小化检测，消耗一帧计数
            if self.restoring_from_tray_frames > 0 {
                self.restoring_from_tray_frames -= 1;
                return;
            }

            let is_minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
            if is_minimized && !self.viewport_was_minimized {
                // 窗口刚进入最小化：触发托盘最小化逻辑
                self.minimize_to_tray(ctx);
            } else if is_minimized && self.viewport_was_minimized && !self.taskbar_hidden {
                // 窗口已处于最小化状态（命令已生效）且任务栏按钮尚未隐藏：
                // 此时安全地隐藏任务栏按钮（只执行一次）
                self.hide_taskbar_button();
                self.taskbar_hidden = true;
            } else if !is_minimized {
                // 窗口已恢复：重置标志
                self.taskbar_hidden = false;
            }
            self.viewport_was_minimized = is_minimized;
        }

        if !self.allow_window_close && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_exit_confirm_dialog = true;
        }
    }

    fn show_exit_confirm_window(&mut self, ctx: &egui::Context) {
        if !self.show_exit_confirm_dialog {
            return;
        }

        let mut open = true;
        let mut exit_app = false;
        let mut minimize_to_tray = false;
        let mut cancel = false;
        let tray_enabled = self.tray.is_some();

        egui::Window::new("确认关闭")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([360.0, 0.0])
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(RichText::new("确定要关闭 WC Notice 吗？").strong());
                if tray_enabled {
                    ui.label(
                        RichText::new("你也可以最小化到托盘，提醒会继续运行。")
                            .color(color_text_muted()),
                    );
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if tray_enabled && ui.button("最小化到托盘").clicked() {
                        minimize_to_tray = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("退出程序").color(color_danger_text()))
                                .fill(color_danger_fill())
                                .stroke(Stroke::new(1.0, color_danger_border())),
                        )
                        .clicked()
                    {
                        exit_app = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });

        if !open || cancel {
            self.show_exit_confirm_dialog = false;
            self.allow_window_close = false;
        }

        if minimize_to_tray {
            self.show_exit_confirm_dialog = false;
            self.allow_window_close = false;
            self.minimize_to_tray(ctx);
        }

        if exit_app {
            self.show_exit_confirm_dialog = false;
            self.allow_window_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn show_top_panel(&mut self, ctx: &egui::Context, now: NaiveTime) {
        let schedule_name = self
            .active_schedule()
            .map(|schedule| schedule.name.clone())
            .unwrap_or_else(|| "无活动时间表".to_string());

        let current_status = self
            .active_schedule()
            .map(|schedule| schedule.current_status(&now))
            .unwrap_or_else(|| "请新建时间表".to_string());

        let next_desc = self
            .active_schedule()
            .and_then(|schedule| {
                schedule
                    .next_period(&now)
                    .and_then(|period| period.naive_time().map(|time| (period.name.clone(), time)))
            })
            .map(|(name, time)| {
                let diff = (time - now).num_seconds().max(0);
                format!("{} · {}", name, format_countdown(diff))
            })
            .unwrap_or_else(|| "今日无后续节点".to_string());

        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::new()
                    .fill(color_panel())
                    .stroke(Stroke::new(1.0, color_border()))
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // ── 左栏：时钟 + 时间表名，居左 ──
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(Local::now().format("%H:%M:%S").to_string())
                                .monospace()
                                .size(22.0)
                                .strong()
                                .color(color_text_strong()),
                        );
                        ui.label(
                            RichText::new(&schedule_name)
                                .size(12.0)
                                .color(color_text_muted()),
                        );
                    });

                    // ── 右栏（含中栏）：right_to_left 布局 ──
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        // 右侧按钮组（right_to_left 顺序：最先添加的在最右）
                        let enabled = self.engine.is_enabled();
                        let (toggle_icon, toggle_fill, toggle_text_color) = if enabled {
                            ("⏸", color_warning_fill(), color_warning_text())
                        } else {
                            ("▶", color_success_fill(), color_success_text())
                        };
                        let toggle_tooltip = if enabled { "暂停" } else { "继续" };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(toggle_icon)
                                        .size(16.0)
                                        .color(toggle_text_color),
                                )
                                .fill(toggle_fill)
                                .stroke(Stroke::new(1.0, color_border()))
                                .corner_radius(8)
                                .min_size(egui::vec2(32.0, 32.0)),
                            )
                            .on_hover_text(toggle_tooltip)
                            .clicked()
                        {
                            let new_state = self.engine.toggle_enabled();
                            self.status_msg = if new_state {
                                "提醒已恢复".to_string()
                            } else {
                                "提醒已暂停".to_string()
                            };
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("🔔").size(16.0))
                                    .fill(color_chip())
                                    .stroke(Stroke::new(1.0, color_border()))
                                    .corner_radius(8)
                                    .min_size(egui::vec2(32.0, 32.0)),
                            )
                            .on_hover_text("音效设置")
                            .clicked()
                        {
                            self.show_sound_window = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("➕").size(16.0))
                                    .fill(color_chip())
                                    .stroke(Stroke::new(1.0, color_border()))
                                    .corner_radius(8)
                                    .min_size(egui::vec2(32.0, 32.0)),
                            )
                            .on_hover_text("新建时间表")
                            .clicked()
                        {
                            self.show_new_schedule_window = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("📋").size(16.0))
                                    .fill(color_chip())
                                    .stroke(Stroke::new(1.0, color_border()))
                                    .corner_radius(8)
                                    .min_size(egui::vec2(32.0, 32.0)),
                            )
                            .on_hover_text("切换/重命名时间表")
                            .clicked()
                        {
                            self.show_schedule_window = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("⚙").size(16.0))
                                    .fill(color_chip())
                                    .stroke(Stroke::new(1.0, color_border()))
                                    .corner_radius(8)
                                    .min_size(egui::vec2(32.0, 32.0)),
                            )
                            .on_hover_text("设置")
                            .clicked()
                        {
                            self.show_settings_window = true;
                        }

                        // 中栏：chip 居中（在 right_to_left 中，这部分在按钮左边）
                        ui.with_layout(
                            egui::Layout::left_to_right(Align::Center).with_main_justify(true),
                            |ui| {
                                ui.horizontal(|ui| {
                                    summary_chip_truncated(
                                        ui,
                                        "当前状态",
                                        &current_status,
                                        color_success_text(),
                                        180.0,
                                    );
                                    summary_chip_truncated(
                                        ui,
                                        "下一节点",
                                        &next_desc,
                                        color_warning_text(),
                                        180.0,
                                    );
                                });
                            },
                        );
                    });
                });
            });
    }

    fn show_schedule_management(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
            let schedules: Vec<(u64, String)> = self
                .config
                .schedules
                .iter()
                .map(|schedule| (schedule.id, schedule.name.clone()))
                .collect();

            ui.horizontal(|ui| {
                ui.label(RichText::new("当前时间表").color(color_text_muted()));

                let mut selected = self.config.active_schedule_id;
                let selected_text = self
                    .active_schedule()
                    .map(|schedule| schedule.name.as_str())
                    .unwrap_or("(无)");

                egui::ComboBox::from_id_salt("active_schedule")
                    .selected_text(selected_text)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (id, name) in &schedules {
                            ui.selectable_value(&mut selected, Some(*id), name);
                        }
                    });

                if selected != self.config.active_schedule_id {
                    self.config.set_active_schedule(selected);
                    self.sync_rename_name_from_active();
                    self.mark_dirty("已切换时间表");
                }

                ui.label(
                    RichText::new(format!("共 {} 个", self.config.schedules.len()))
                        .size(12.0)
                        .color(color_text_muted()),
                );
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("重命名").color(color_text_muted()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.rename_schedule_name)
                        .desired_width(220.0)
                        .hint_text(RichText::new("当前时间表名称").color(color_hint_text())),
                );

                if ui.button("√ 改名").clicked() {
                    let new_name = self.rename_schedule_name.trim().to_string();
                    if new_name.is_empty() {
                        self.status_msg = "时间表名称不能为空".to_string();
                    } else if let Some(schedule) = self.active_schedule_mut() {
                        schedule.name = new_name;
                        self.sync_rename_name_from_active();
                        self.mark_dirty("时间表已重命名");
                    }
                }

                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("🗑 删除该时间表").color(color_danger_text()),
                        )
                        .fill(color_danger_fill())
                        .stroke(Stroke::new(1.0, color_danger_border())),
                    )
                    .clicked()
                {
                    if self.config.remove_active_schedule().is_some() {
                        self.sync_rename_name_from_active();
                        self.mark_dirty("时间表已删除");
                    }
                }
            });
        });
    }

    fn show_new_schedule(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("名称").color(color_text_muted()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_schedule_name)
                        .desired_width(220.0)
                        .hint_text(RichText::new("输入新时间表名称").color(color_hint_text())),
                );

                if ui.button("√ 创建").clicked() {
                    let name = self.new_schedule_name.trim();
                    let final_name = if name.is_empty() {
                        format!("时间表{}", self.config.next_schedule_id)
                    } else {
                        name.to_string()
                    };

                    self.config.create_empty_schedule(final_name);
                    self.new_schedule_name.clear();
                    self.sync_rename_name_from_active();
                    self.mark_dirty("新时间表已创建");
                }
            });
        });
    }

    fn show_sound_settings(&mut self, ui: &mut Ui) {
        let mut changed = false;

        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
            let active_name = self
                .active_schedule()
                .map(|schedule| schedule.name.clone())
                .unwrap_or_else(|| "(无)".to_string());

            ui.label(
                RichText::new(format!("当前时间表: {active_name}"))
                    .size(13.0)
                    .color(color_text_muted()),
            );

            if let Some(schedule) = self.active_schedule_mut() {
                changed |= draw_sound_source_editor(
                    ui,
                    "开始音效",
                    &format!("sound_start_{}", schedule.id),
                    &mut schedule.sound.start,
                    PeriodKind::Start,
                );
                ui.add_space(6.0);
                changed |= draw_sound_source_editor(
                    ui,
                    "结束音效",
                    &format!("sound_end_{}", schedule.id),
                    &mut schedule.sound.end,
                    PeriodKind::End,
                );
            }
        });

        if changed {
            self.mark_dirty("音效设置已保存");
        }
    }

    fn show_period_editor(&mut self, ui: &mut Ui, now: NaiveTime) {
        let added = false;
        let mut changed_existing = false;

        card_no_title(ui, |ui| {
            // "+" 按钮居中，点击后打开弹窗
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("  +  ")
                                .size(20.0)
                                .strong()
                                .color(color_text_strong()),
                        )
                        .fill(color_chip())
                        .stroke(Stroke::new(1.5, color_border()))
                        .corner_radius(8)
                        .min_size(egui::vec2(48.0, 36.0)),
                    )
                    .on_hover_text("添加时间节点")
                    .clicked()
                {
                    self.show_add_dialog = true;
                }
            });

            ui.add_space(8.0);

            if let Some(schedule) = self.active_schedule_mut() {
                if schedule.periods.is_empty() {
                    ui.label(
                        RichText::new("当前时间表没有节点，请先添加开始/结束节点")
                            .color(color_text_muted()),
                    );
                    return;
                }

                let mut delete_index: Option<usize> = None;

                for (idx, period) in schedule.periods.iter_mut().enumerate() {
                    let (row_fill, row_border) = period_row_style(period, &now);
                    egui::Frame::new()
                        .fill(row_fill)
                        .stroke(Stroke::new(1.0, row_border))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            let row_width = ui.available_width();
                            ui.allocate_ui_with_layout(
                                egui::vec2(row_width, PERIOD_ROW_MIN_HEIGHT),
                                egui::Layout::left_to_right(egui::Align::Center)
                                    .with_main_justify(false),
                                |ui| {
                                    if ui.checkbox(&mut period.enabled, "").changed() {
                                        changed_existing = true;
                                    }

                                    let time_response = ui.add_sized(
                                        [PERIOD_TIME_WIDTH, 24.0],
                                        egui::TextEdit::singleline(&mut period.time),
                                    );
                                    if time_response.changed() {
                                        changed_existing = true;
                                    }
                                    // 失去焦点时规范化时间格式
                                    if time_response.lost_focus() {
                                        if let Some(normalized) =
                                            schedule::normalize_time_str(&period.time)
                                        {
                                            period.time = normalized;
                                            changed_existing = true;
                                        }
                                        // 如果格式无效，保留原值（用户可继续编辑）
                                    }

                                    let mut kind = period.kind;
                                    egui::ComboBox::from_id_salt(format!(
                                        "period_kind_{}_{}",
                                        schedule.id, idx
                                    ))
                                    .selected_text(kind.label())
                                    .width(PERIOD_KIND_WIDTH)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut kind, PeriodKind::Start, "开始");
                                        ui.selectable_value(&mut kind, PeriodKind::End, "结束");
                                    });

                                    if kind != period.kind {
                                        period.kind = kind;
                                        changed_existing = true;
                                    }

                                    let reserved_tail = PERIOD_STATUS_WIDTH
                                        + PERIOD_DELETE_WIDTH
                                        + ui.spacing().item_spacing.x * 2.0;
                                    let name_width = (ui.available_width() - reserved_tail)
                                        .max(PERIOD_NAME_MIN_WIDTH);

                                    if ui
                                        .add_sized(
                                            [name_width, 24.0],
                                            egui::TextEdit::singleline(&mut period.name),
                                        )
                                        .changed()
                                    {
                                        changed_existing = true;
                                    }

                                    ui.add_sized(
                                        [PERIOD_STATUS_WIDTH, 24.0],
                                        egui::Label::new(
                                            RichText::new(period_runtime_state(period, &now))
                                                .size(12.0)
                                                .color(color_text_muted()),
                                        ),
                                    );

                                    if ui
                                        .add_sized(
                                            [PERIOD_DELETE_WIDTH, 24.0],
                                            egui::Button::new(
                                                RichText::new("删除").color(color_danger_text()),
                                            )
                                            .fill(color_danger_fill())
                                            .stroke(Stroke::new(1.0, color_danger_border())),
                                        )
                                        .clicked()
                                    {
                                        delete_index = Some(idx);
                                    }
                                },
                            );
                        });
                    ui.add_space(4.0);
                }

                if let Some(idx) = delete_index {
                    schedule.periods.remove(idx);
                    changed_existing = true;
                }

                if changed_existing {
                    schedule.sort_periods();
                }
            }
        });

        if added {
            self.mark_dirty("新节点已添加");
        } else if changed_existing {
            self.mark_dirty("时间节点已更新");
        }
    }
}

impl eframe::App for WcNoticeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_applied {
            apply_theme(ctx);
            self.theme_applied = true;
        }

        self.flush_pending_save();
        self.handle_tray_events(ctx);
        self.handle_window_lifecycle(ctx);

        for event in self.engine.take_status_events() {
            self.status_msg = event;
        }

        self.sync_rename_name_from_active();

        let now = Local::now().naive_local().time();
        self.show_top_panel(ctx, now);

        // 底部状态栏（必须在 CentralPanel 之前声明）
        let status_msg_clone = self.status_msg.clone();
        let cfg_path = crate::config::config_path().display().to_string();
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(220, 224, 216))
                    .stroke(Stroke::new(1.0, color_border()))
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // 左侧：状态信息
                    ui.label(
                        RichText::new(&status_msg_clone)
                            .font(FontId::proportional(11.0))
                            .color(status_color(&status_msg_clone)),
                    );

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        // 右侧：配置路径（截短显示，hover 显示完整路径）
                        let short_path = shorten_path(&cfg_path, 60);
                        let resp = ui.label(
                            RichText::new(format!("配置文件 {short_path}"))
                                .font(FontId::proportional(11.0))
                                .color(color_text_muted()),
                        );
                        if short_path.len() < cfg_path.len() {
                            resp.on_hover_text(&cfg_path);
                        }
                    });
                });
            });

        // 切换/重命名时间表弹窗
        let mut show_schedule_window = self.show_schedule_window;
        if show_schedule_window {
            egui::Window::new("切换 / 重命名时间表")
                .open(&mut show_schedule_window)
                .fixed_size([480.0, 0.0])
                .collapsible(false)
                .show(ctx, |ui| {
                    self.show_schedule_management(ui);
                });
        }
        self.show_schedule_window = show_schedule_window;

        // 新建时间表弹窗
        let mut show_new_schedule_window = self.show_new_schedule_window;
        if show_new_schedule_window {
            egui::Window::new("新建时间表")
                .open(&mut show_new_schedule_window)
                .fixed_size([400.0, 0.0])
                .collapsible(false)
                .show(ctx, |ui| {
                    self.show_new_schedule(ui);
                });
        }
        self.show_new_schedule_window = show_new_schedule_window;

        // 音效设置弹窗
        let mut show_sound_window = self.show_sound_window;
        if show_sound_window {
            egui::Window::new("音效设置")
                .open(&mut show_sound_window)
                .fixed_size([480.0, 0.0])
                .collapsible(false)
                .show(ctx, |ui| {
                    self.show_sound_settings(ui);
                });
        }
        self.show_sound_window = show_sound_window;

        // 设置窗口
        if self.show_settings_window {
            let mut open = true;
            egui::Window::new("设置")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .fixed_size([300.0, 80.0])
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        let mut autostart = self.config.autostart;
                        if ui.checkbox(&mut autostart, "开机自动启动").changed() {
                            self.config.autostart = autostart;
                            self.apply_autostart();
                            self.mark_dirty("设置已保存");
                        }
                    });
                    ui.add_space(8.0);
                });
            if !open {
                self.show_settings_window = false;
            }
        }

        // 新增节点弹窗
        if self.show_add_dialog {
            let mut open = true;
            let mut do_add = false;
            let mut do_cancel = false;

            egui::Window::new("添加时间节点")
                .open(&mut open)
                .fixed_size([380.0, 0.0])
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("时间").color(color_text_muted()));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_period_time)
                                    .desired_width(100.0)
                                    .hint_text(RichText::new("HH:MM:SS").color(color_hint_text())),
                            );
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("类型").color(color_text_muted()));
                            egui::ComboBox::from_id_salt("dialog_period_kind")
                                .selected_text(self.new_period_kind.label())
                                .width(100.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.new_period_kind,
                                        PeriodKind::Start,
                                        "开始",
                                    );
                                    ui.selectable_value(
                                        &mut self.new_period_kind,
                                        PeriodKind::End,
                                        "结束",
                                    );
                                });
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("名称").color(color_text_muted()));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_period_name)
                                    .desired_width(240.0)
                                    .hint_text(
                                        RichText::new("例如：第1节开始").color(color_hint_text()),
                                    ),
                            );
                        });

                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            if ui.button("✔ 确认添加").clicked() {
                                do_add = true;
                            }
                            if ui.button("✖ 取消").clicked() {
                                do_cancel = true;
                            }
                        });
                    });
                });

            if !open || do_cancel {
                self.show_add_dialog = false;
            }

            if do_add {
                let time = self.new_period_time.trim().to_string();
                let name = self.new_period_name.trim().to_string();
                let kind = self.new_period_kind;

                match schedule::normalize_time_str(&time) {
                    None => {
                        self.status_msg =
                            "时间格式错误，请使用 HH:MM:SS（时0-23，分/秒0-59）".to_string();
                    }
                    Some(normalized_time) => {
                        if name.is_empty() {
                            self.status_msg = "节点名称不能为空".to_string();
                        } else if let Some(schedule) = self.active_schedule_mut() {
                            schedule
                                .periods
                                .push(Period::new(&normalized_time, kind, &name));
                            schedule.sort_periods();
                            self.show_add_dialog = false;
                            self.mark_dirty("新节点已添加");
                        }
                    }
                }
            }
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(color_background())
                    .inner_margin(egui::Margin::symmetric(12, 12)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::both()
                    .id_salt("main_content_scroll")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width().max(MIN_CONTENT_WIDTH));

                        if self.active_schedule().is_some() {
                            self.show_period_editor(ui, now);
                        } else {
                            card(ui, "空状态", |ui| {
                                ui.label(
                                    RichText::new("当前没有任何时间表，请先点击顶部「➕」按钮创建一个空时间表")
                                        .size(14.0)
                                        .color(color_text_muted()),
                                );
                            });
                        }
                    });
            });

        self.show_exit_confirm_window(ctx);

        // 有 pending 时用 200ms 刷新确保防抖及时触发，否则 1s 刷新即可
        let repaint_delay = if self.pending_save.is_some() {
            Duration::from_millis(200)
        } else {
            Duration::from_secs(1)
        };
        ctx.request_repaint_after(repaint_delay);
    }
}

fn draw_sound_source_editor(
    ui: &mut Ui,
    label: &str,
    id_base: &str,
    source: &mut SoundSource,
    kind: PeriodKind,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(14.0)
                .strong()
                .color(color_text_strong()),
        );

        let is_builtin = matches!(source, SoundSource::Builtin(_));

        if ui.selectable_label(is_builtin, "内置").clicked() && !is_builtin {
            *source = SoundSource::Builtin(kind.default_builtin_sound());
            changed = true;
        }

        if ui.selectable_label(!is_builtin, "本地").clicked() && is_builtin {
            *source = SoundSource::Local {
                path: String::new(),
            };
            changed = true;
        }
    });

    ui.horizontal(|ui| match source {
        SoundSource::Builtin(sound) => {
            let mut selected = *sound;
            egui::ComboBox::from_id_salt(format!("{}_builtin", id_base))
                .selected_text(selected.label())
                .width(180.0)
                .show_ui(ui, |ui| {
                    for builtin in BuiltinSound::ALL {
                        ui.selectable_value(&mut selected, builtin, builtin.label());
                    }
                });

            if selected != *sound {
                *sound = selected;
                changed = true;
            }
        }
        SoundSource::Local { path } => {
            if ui
                .add(
                    egui::TextEdit::singleline(path)
                        .desired_width(340.0)
                        .hint_text(
                            RichText::new("本地音效绝对路径 (*.mp3; *.wav)")
                                .color(color_hint_text()),
                        ),
                )
                .changed()
            {
                changed = true;
            }

            if ui.button("浏览").clicked() {
                if let Some(file) = FileDialog::new()
                    .add_filter("Audio", &["mp3", "wav"])
                    .pick_file()
                {
                    let abs = make_abs_path(file);
                    *path = abs.display().to_string();
                    changed = true;
                }
            }
        }
    });

    changed
}

fn make_abs_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }

    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path,
    }
}

fn period_runtime_state(period: &Period, now: &NaiveTime) -> &'static str {
    if !period.enabled {
        return "停用";
    }

    if period.matches_now(now) {
        return "当前";
    }

    if period.naive_time().map(|time| time < *now).unwrap_or(false) {
        return "已过";
    }

    "未到"
}

fn period_row_style(period: &Period, now: &NaiveTime) -> (Color32, Color32) {
    let is_past = period.naive_time().map(|time| time < *now).unwrap_or(false);

    // 已过和停用统一淡灰，减少噪声，突出即将发生/当前节点
    if !period.enabled || is_past {
        return (color_period_past_fill(), color_period_past_border());
    }

    let is_current = period.matches_now(now);
    match period.kind {
        PeriodKind::Start => {
            if is_current {
                (
                    color_period_start_current_fill(),
                    color_period_start_current_border(),
                )
            } else {
                (color_period_start_fill(), color_period_start_border())
            }
        }
        PeriodKind::End => {
            if is_current {
                (
                    color_period_end_current_fill(),
                    color_period_end_current_border(),
                )
            } else {
                (color_period_end_fill(), color_period_end_border())
            }
        }
    }
}

fn card<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = egui::Frame::new()
        .fill(color_surface())
        .stroke(Stroke::new(1.0, color_border()))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .size(15.0)
                    .strong()
                    .color(color_text_strong()),
            );
            ui.add_space(8.0);
            add_contents(ui)
        });
    inner.inner
}

/// 无标题的卡片容器，内容填满可用宽度
fn card_no_title<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = egui::Frame::new()
        .fill(color_surface())
        .stroke(Stroke::new(1.0, color_border()))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        });
    inner.inner
}

/// 带宽度限制的 chip：value 超出时截断并追加 "…"，不换行
fn summary_chip_truncated(
    ui: &mut Ui,
    title: &str,
    value: &str,
    value_color: Color32,
    max_width: f32,
) {
    egui::Frame::new()
        .fill(color_chip())
        .stroke(Stroke::new(1.0, color_border()))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(9, 6))
        .show(ui, |ui| {
            // frame 内部宽度 = max_width - 左右 margin(9*2)
            let inner_w = (max_width - 18.0).max(20.0);
            ui.set_max_width(inner_w);

            ui.label(
                RichText::new(title)
                    .size(11.0)
                    .strong()
                    .color(color_text_muted()),
            );

            // 用 galley 测量文字宽度，超出则逐字符截断
            let font_id = egui::FontId::proportional(13.0);
            let full_text = value.to_string();
            let galley =
                ui.fonts(|f| f.layout_no_wrap(full_text.clone(), font_id.clone(), value_color));

            let display_text = if galley.rect.width() <= inner_w {
                full_text
            } else {
                // 二分或线性截断，找到最长可放入的前缀
                let chars: Vec<char> = value.chars().collect();
                let mut lo = 0usize;
                let mut hi = chars.len();
                while lo + 1 < hi {
                    let mid = (lo + hi) / 2;
                    let candidate: String = chars[..mid].iter().collect::<String>() + "…";
                    let g = ui.fonts(|f| f.layout_no_wrap(candidate, font_id.clone(), value_color));
                    if g.rect.width() <= inner_w {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                chars[..lo].iter().collect::<String>() + "…"
            };

            ui.label(
                RichText::new(display_text)
                    .size(13.0)
                    .strong()
                    .color(value_color),
            )
            .on_hover_text(value); // hover 显示完整内容
        });
}

fn format_countdown(diff_secs: i64) -> String {
    let h = diff_secs / 3600;
    let m = (diff_secs % 3600) / 60;
    let s = diff_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();

    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size = egui::vec2(44.0, 30.0);

    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(24.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );

    style.visuals.panel_fill = color_background();
    style.visuals.window_fill = color_surface();
    style.visuals.override_text_color = Some(color_text_strong());
    style.visuals.window_corner_radius = egui::CornerRadius::same(8);

    ctx.set_style(style);
}

fn status_color(status_msg: &str) -> Color32 {
    if status_msg.contains("失败") || status_msg.contains("错误") {
        color_danger_text()
    } else if status_msg.contains("暂停") {
        color_warning_text()
    } else {
        color_text_muted()
    }
}

fn color_background() -> Color32 {
    Color32::from_rgb(243, 245, 240)
}

fn color_panel() -> Color32 {
    Color32::from_rgb(236, 239, 233)
}

fn color_surface() -> Color32 {
    Color32::from_rgb(250, 251, 247)
}

fn color_chip() -> Color32 {
    Color32::from_rgb(240, 244, 236)
}

fn color_period_start_fill() -> Color32 {
    Color32::from_rgb(235, 246, 234)
}

fn color_period_start_border() -> Color32 {
    Color32::from_rgb(181, 207, 178)
}

fn color_period_start_current_fill() -> Color32 {
    Color32::from_rgb(223, 239, 221)
}

fn color_period_start_current_border() -> Color32 {
    Color32::from_rgb(144, 182, 141)
}

fn color_period_end_fill() -> Color32 {
    Color32::from_rgb(248, 240, 228)
}

fn color_period_end_border() -> Color32 {
    Color32::from_rgb(220, 198, 164)
}

fn color_period_end_current_fill() -> Color32 {
    Color32::from_rgb(245, 231, 214)
}

fn color_period_end_current_border() -> Color32 {
    Color32::from_rgb(205, 170, 122)
}

fn color_period_past_fill() -> Color32 {
    Color32::from_rgb(239, 241, 239)
}

fn color_period_past_border() -> Color32 {
    Color32::from_rgb(212, 216, 211)
}

fn color_border() -> Color32 {
    Color32::from_rgb(206, 212, 201)
}

fn color_text_strong() -> Color32 {
    Color32::from_rgb(43, 50, 44)
}

fn color_text_muted() -> Color32 {
    Color32::from_rgb(104, 112, 103)
}

fn color_success_text() -> Color32 {
    Color32::from_rgb(52, 111, 72)
}

fn color_success_fill() -> Color32 {
    Color32::from_rgb(223, 237, 223)
}

fn color_warning_text() -> Color32 {
    Color32::from_rgb(166, 96, 45)
}

fn color_warning_fill() -> Color32 {
    Color32::from_rgb(245, 231, 219)
}

fn color_danger_text() -> Color32 {
    Color32::from_rgb(151, 70, 65)
}

fn color_danger_fill() -> Color32 {
    Color32::from_rgb(247, 228, 226)
}

fn color_danger_border() -> Color32 {
    Color32::from_rgb(214, 176, 173)
}

fn color_hint_text() -> Color32 {
    Color32::from_rgb(180, 185, 178)
}

/// 若路径字符数超过 `max_chars`，从头部截断并加 "…" 前缀
fn shorten_path(path: &str, max_chars: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max_chars {
        path.to_string()
    } else {
        let keep = &chars[chars.len() - max_chars..];
        format!("…{}", keep.iter().collect::<String>())
    }
}
