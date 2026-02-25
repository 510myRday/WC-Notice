use chrono::Local;
use eframe::egui;
use eframe::egui::{Color32, RichText, Ui};
use std::sync::Arc;

use crate::config::save_schedule;
use crate::engine::Engine;
use crate::schedule::{Period, PeriodType, Schedule};

pub struct WcNoticeApp {
    engine: Arc<Engine>,
    schedule: Schedule,
    /// 新增节点的临时表单
    new_period_time: String,
    new_period_name: String,
    new_period_type: PeriodType,
    /// 状态栏消息
    status_msg: String,
    /// 强制 UI 每秒刷新
    last_tick: std::time::Instant,
}

impl WcNoticeApp {
    pub fn new(engine: Arc<Engine>, schedule: Schedule) -> Self {
        Self {
            engine,
            schedule,
            new_period_time: "09:00".to_string(),
            new_period_name: "自定义节点".to_string(),
            new_period_type: PeriodType::Custom,
            status_msg: "就绪".to_string(),
            last_tick: std::time::Instant::now(),
        }
    }
}

impl eframe::App for WcNoticeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 每秒刷新一次 UI（保持倒计时实时更新）
        if self.last_tick.elapsed().as_secs() >= 1 {
            self.last_tick = std::time::Instant::now();
            ctx.request_repaint();
        }

        let now = Local::now().naive_local().time();

        // ── 顶部状态栏 ──
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🔔 WC Notice");
                ui.separator();
                let enabled = self.engine.is_enabled();
                let btn_text = if enabled {
                    "⏸ 暂停提醒"
                } else {
                    "▶ 启用提醒"
                };
                let btn_color = if enabled {
                    Color32::from_rgb(80, 180, 80)
                } else {
                    Color32::from_rgb(200, 80, 80)
                };
                if ui
                    .button(RichText::new(btn_text).color(btn_color))
                    .clicked()
                {
                    let new_state = self.engine.toggle_enabled();
                    self.status_msg = if new_state {
                        "✅ 提醒已启用".into()
                    } else {
                        "⏸ 提醒已暂停".into()
                    };
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("🕐 {}", Local::now().format("%H:%M:%S"))).size(16.0),
                    );
                });
            });
        });

        // ── 底部状态栏 ──
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_msg);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let config_path = crate::config::config_path();
                    ui.label(RichText::new(format!("配置: {}", config_path.display())).weak());
                });
            });
        });

        // ── 左侧：当前状态面板 ──
        egui::SidePanel::left("status_panel")
            .min_width(200.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("当前状态").strong());
                ui.separator();

                let current = self.schedule.current_status(&now);
                ui.label(
                    RichText::new(&current)
                        .size(18.0)
                        .color(Color32::from_rgb(100, 200, 100)),
                );

                ui.add_space(8.0);
                ui.label(RichText::new("下一节点").strong());
                ui.separator();
                if let Some(next) = self.schedule.next_period(&now) {
                    if let Some(nt) = next.naive_time() {
                        let diff_secs = (nt - now).num_seconds();
                        let h = diff_secs / 3600;
                        let m = (diff_secs % 3600) / 60;
                        let s = diff_secs % 60;
                        ui.label(RichText::new(&next.name).size(15.0));
                        ui.label(
                            RichText::new(format!("⏳ {:02}:{:02}:{:02}", h, m, s))
                                .size(20.0)
                                .color(Color32::from_rgb(255, 200, 80)),
                        );
                    }
                } else {
                    ui.label("今天的课程已全部结束 🎉");
                }

                ui.add_space(16.0);
                ui.label(RichText::new("时间表").strong());
                ui.label(RichText::new(&self.schedule.name).weak());
            });

        // ── 中央：时间表编辑面板 ──
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("作息时间表");
            ui.separator();

            // 滚动列表
            let mut delete_index: Option<usize> = None;
            egui::ScrollArea::vertical()
                .max_height(400.0)
                .show(ui, |ui| {
                    for (i, period) in self.schedule.periods.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            // 启用开关
                            ui.checkbox(&mut period.enabled, "");
                            // 时间
                            ui.label(RichText::new(&period.time).monospace().size(14.0));
                            // 类型标签
                            let type_color = period_type_color(&period.period_type);
                            ui.label(RichText::new(period.period_type.label()).color(type_color));
                            // 名称
                            ui.label(&period.name);
                            // 高亮当前
                            if period.matches_now(&now) {
                                ui.label(RichText::new("← 当前").color(Color32::YELLOW));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("🗑").clicked() {
                                        delete_index = Some(i);
                                    }
                                },
                            );
                        });
                    }
                });
            if let Some(i) = delete_index {
                self.schedule.periods.remove(i);
                self.engine.update_schedule(self.schedule.clone());
            }

            ui.separator();
            // 新增节点区域
            show_add_period_form(ui, self);
        });

        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}

fn show_add_period_form(ui: &mut Ui, app: &mut WcNoticeApp) {
    ui.collapsing("➕ 添加新节点", |ui| {
        ui.horizontal(|ui| {
            ui.label("时间 (HH:MM):");
            ui.text_edit_singleline(&mut app.new_period_time);
        });
        ui.horizontal(|ui| {
            ui.label("名称:");
            ui.text_edit_singleline(&mut app.new_period_name);
        });
        ui.horizontal(|ui| {
            ui.label("类型:");
            egui::ComboBox::from_id_salt("period_type")
                .selected_text(app.new_period_type.label())
                .show_ui(ui, |ui| {
                    for t in [
                        PeriodType::ClassStart,
                        PeriodType::ClassEnd,
                        PeriodType::Exercise,
                        PeriodType::LunchBreak,
                        PeriodType::EveningStudy,
                        PeriodType::EveningEnd,
                        PeriodType::Custom,
                    ] {
                        let label = t.label().to_string();
                        ui.selectable_value(&mut app.new_period_type, t.clone(), label);
                    }
                });
        });
        if ui.button("添加").clicked() {
            let p = Period::new(
                &app.new_period_time.clone(),
                app.new_period_type.clone(),
                &app.new_period_name.clone(),
            );
            app.schedule.periods.push(p);
            // 按时间排序
            app.schedule.periods.sort_by(|a, b| a.time.cmp(&b.time));
            app.engine.update_schedule(app.schedule.clone());
            match save_schedule(&app.schedule) {
                Ok(_) => app.status_msg = "✅ 已保存".into(),
                Err(e) => app.status_msg = format!("❌ 保存失败: {}", e),
            }
        }
        if ui.button("💾 保存时间表").clicked() {
            match save_schedule(&app.schedule) {
                Ok(_) => app.status_msg = "✅ 时间表已保存".into(),
                Err(e) => app.status_msg = format!("❌ 保存失败: {}", e),
            }
        }
    });
}

fn period_type_color(t: &PeriodType) -> Color32 {
    match t {
        PeriodType::ClassStart | PeriodType::EveningStudy => Color32::from_rgb(80, 160, 255),
        PeriodType::ClassEnd | PeriodType::EveningEnd => Color32::from_rgb(255, 140, 80),
        PeriodType::Exercise => Color32::from_rgb(80, 220, 120),
        PeriodType::LunchBreak => Color32::from_rgb(255, 210, 80),
        PeriodType::Custom => Color32::from_rgb(180, 180, 180),
    }
}
