use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{Local, Timelike};

use crate::notifier::{play_sound_for_period, send_notification};
use crate::schedule::AppConfig;

/// 时间检测引擎
pub struct Engine {
    pub config: Arc<Mutex<AppConfig>>,
    pub enabled: Arc<Mutex<bool>>,
    /// 上次触发的分钟数（防重复触发）
    last_triggered_minute: Arc<Mutex<Option<u32>>>,
    /// 后台线程向 UI 上报状态消息
    status_events: Arc<Mutex<Vec<String>>>,
}

impl Engine {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            enabled: Arc::new(Mutex::new(true)),
            last_triggered_minute: Arc::new(Mutex::new(None)),
            status_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 启动后台检测线程，每秒检查一次系统时间
    pub fn start(&self) {
        let config = Arc::clone(&self.config);
        let enabled = Arc::clone(&self.enabled);
        let last_triggered = Arc::clone(&self.last_triggered_minute);
        let status_events = Arc::clone(&self.status_events);

        thread::spawn(move || {
            let mut warned_once: HashSet<String> = HashSet::new();
            log::info!("时间检测引擎已启动");

            loop {
                thread::sleep(Duration::from_secs(1));

                if !*enabled.lock().unwrap() {
                    continue;
                }

                let now = Local::now().naive_local().time();
                let current_minute = now.hour() * 60 + now.minute();

                {
                    let last = last_triggered.lock().unwrap();
                    if *last == Some(current_minute) {
                        continue;
                    }
                }

                let triggered = {
                    let cfg = config.lock().unwrap();
                    cfg.active_schedule().and_then(|schedule| {
                        schedule
                            .periods
                            .iter()
                            .find(|period| period.matches_now(&now))
                            .cloned()
                            .map(|period| (period, schedule.sound.clone()))
                    })
                };

                if let Some((period, sound_slots)) = triggered {
                    log::info!("命中节点: {} - {}", period.name, period.kind.label());

                    if let Some(warning) = play_sound_for_period(period.kind, &sound_slots) {
                        if warned_once.insert(warning.clone()) {
                            status_events.lock().unwrap().push(warning);
                        }
                    }

                    send_notification(&format!("🔔 {}", period.kind.label()), &period.name);

                    let mut last = last_triggered.lock().unwrap();
                    *last = Some(current_minute);
                }
            }
        });
    }

    pub fn update_config(&self, new_config: AppConfig) {
        let mut cfg = self.config.lock().unwrap();
        *cfg = new_config;
    }

    pub fn toggle_enabled(&self) -> bool {
        let mut enabled = self.enabled.lock().unwrap();
        *enabled = !*enabled;
        *enabled
    }

    pub fn is_enabled(&self) -> bool {
        *self.enabled.lock().unwrap()
    }

    pub fn take_status_events(&self) -> Vec<String> {
        let mut events = self.status_events.lock().unwrap();
        std::mem::take(&mut *events)
    }
}
