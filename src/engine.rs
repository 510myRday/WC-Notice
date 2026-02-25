use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{Local, Timelike};

use crate::notifier::{play_bell, send_notification};
use crate::schedule::Schedule;

/// 时间检测引擎
pub struct Engine {
    pub schedule: Arc<Mutex<Schedule>>,
    pub enabled: Arc<Mutex<bool>>,
    /// 上次触发的分钟数（防重复触发）
    last_triggered_minute: Arc<Mutex<Option<u32>>>,
}

impl Engine {
    pub fn new(schedule: Schedule) -> Self {
        Self {
            schedule: Arc::new(Mutex::new(schedule)),
            enabled: Arc::new(Mutex::new(true)),
            last_triggered_minute: Arc::new(Mutex::new(None)),
        }
    }

    /// 启动后台检测线程，每秒检查一次系统时间
    pub fn start(&self) {
        let schedule = Arc::clone(&self.schedule);
        let enabled = Arc::clone(&self.enabled);
        let last_triggered = Arc::clone(&self.last_triggered_minute);

        thread::spawn(move || {
            log::info!("时间检测引擎已启动");
            loop {
                thread::sleep(Duration::from_secs(1));

                // 未启用则跳过
                if !*enabled.lock().unwrap() {
                    continue;
                }

                let now = Local::now().naive_local().time();
                // 当前分钟的唯一 key：hour * 60 + minute
                let current_minute = now.hour() * 60 + now.minute();

                // 防重复：同一分钟只触发一次
                {
                    let last = last_triggered.lock().unwrap();
                    if *last == Some(current_minute) {
                        continue;
                    }
                }

                // 检查时间表中是否有节点命中
                let sched = schedule.lock().unwrap();
                for period in &sched.periods {
                    if period.matches_now(&now) {
                        log::info!("命中节点: {} - {}", period.name, period.period_type.label());

                        // 播放铃声
                        play_bell(&period.period_type.bell_type());

                        // 发送系统通知
                        send_notification(
                            &format!("🔔 {}", period.period_type.label()),
                            &period.name,
                        );

                        // 记录已触发的分钟
                        let mut last = last_triggered.lock().unwrap();
                        *last = Some(current_minute);
                        break;
                    }
                }
            }
        });
    }

    /// 更新时间表（GUI编辑后调用）
    pub fn update_schedule(&self, new_schedule: Schedule) {
        let mut sched = self.schedule.lock().unwrap();
        *sched = new_schedule;
    }

    /// 切换启用/暂停状态
    pub fn toggle_enabled(&self) -> bool {
        let mut enabled = self.enabled.lock().unwrap();
        *enabled = !*enabled;
        *enabled
    }

    pub fn is_enabled(&self) -> bool {
        *self.enabled.lock().unwrap()
    }
}
