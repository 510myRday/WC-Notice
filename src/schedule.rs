use chrono::{NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeriodKind {
    Start,
    End,
}

impl PeriodKind {
    pub fn label(&self) -> &str {
        match self {
            PeriodKind::Start => "开始",
            PeriodKind::End => "结束",
        }
    }

    pub fn default_builtin_sound(&self) -> BuiltinSound {
        match self {
            PeriodKind::Start => BuiltinSound::BellStart,
            PeriodKind::End => BuiltinSound::BellEnd,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltinSound {
    BellStart,
    BellEnd,
    Fun,
}

impl BuiltinSound {
    pub const ALL: [BuiltinSound; 3] = [
        BuiltinSound::BellStart,
        BuiltinSound::BellEnd,
        BuiltinSound::Fun,
    ];

    pub fn label(&self) -> &str {
        match self {
            BuiltinSound::BellStart => "bell_start.mp3",
            BuiltinSound::BellEnd => "bell_end.mp3",
            BuiltinSound::Fun => "bell_other.mp3",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoundSource {
    Builtin(BuiltinSound),
    Local { path: String },
}

impl SoundSource {
    pub fn default_for_kind(kind: PeriodKind) -> Self {
        SoundSource::Builtin(kind.default_builtin_sound())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundSlots {
    pub start: SoundSource,
    pub end: SoundSource,
}

impl Default for SoundSlots {
    fn default() -> Self {
        Self {
            start: SoundSource::default_for_kind(PeriodKind::Start),
            end: SoundSource::default_for_kind(PeriodKind::End),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Period {
    pub time: String,
    pub kind: PeriodKind,
    pub name: String,
    pub enabled: bool,
}

impl Period {
    pub fn new(time: &str, kind: PeriodKind, name: &str) -> Self {
        Self {
            time: time.to_string(),
            kind,
            name: name.to_string(),
            enabled: true,
        }
    }

    pub fn naive_time(&self) -> Option<NaiveTime> {
        NaiveTime::parse_from_str(&self.time, "%H:%M:%S")
            .or_else(|_| NaiveTime::parse_from_str(&self.time, "%H:%M"))
            .ok()
    }

    pub fn matches_now(&self, now: &NaiveTime) -> bool {
        if !self.enabled {
            return false;
        }

        self.naive_time()
            .map(|time| {
                time.hour() == now.hour()
                    && time.minute() == now.minute()
                    && time.second() == now.second()
            })
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleProfile {
    pub id: u64,
    pub name: String,
    pub periods: Vec<Period>,
    pub sound: SoundSlots,
}

impl ScheduleProfile {
    pub fn default_preset(id: u64) -> Self {
        let periods = vec![
            Period::new("08:00:00", PeriodKind::Start, "第1节开始"),
            Period::new("08:45:00", PeriodKind::End, "第1节结束"),
            Period::new("08:55:00", PeriodKind::Start, "第2节开始"),
            Period::new("09:40:00", PeriodKind::End, "第2节结束"),
            Period::new("10:10:00", PeriodKind::Start, "第3节开始"),
            Period::new("10:55:00", PeriodKind::End, "第3节结束"),
            Period::new("11:05:00", PeriodKind::Start, "第4节开始"),
            Period::new("11:50:00", PeriodKind::End, "上午结束"),
            Period::new("13:50:00", PeriodKind::Start, "第5节开始"),
            Period::new("14:35:00", PeriodKind::End, "第5节结束"),
            Period::new("14:45:00", PeriodKind::Start, "第6节开始"),
            Period::new("15:30:00", PeriodKind::End, "第6节结束"),
            Period::new("15:40:00", PeriodKind::Start, "第7节开始"),
            Period::new("16:25:00", PeriodKind::End, "第7节结束"),
            Period::new("19:00:00", PeriodKind::Start, "晚自习开始"),
            Period::new("21:30:00", PeriodKind::End, "晚自习结束"),
        ];

        Self {
            id,
            name: "默认时间表".to_string(),
            periods,
            sound: SoundSlots::default(),
        }
    }

    pub fn empty(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            periods: Vec::new(),
            sound: SoundSlots::default(),
        }
    }

    pub fn sort_periods(&mut self) {
        self.periods.sort_by(|a, b| a.time.cmp(&b.time));
    }

    pub fn next_period(&self, now: &NaiveTime) -> Option<&Period> {
        self.periods
            .iter()
            .filter(|period| period.enabled)
            .filter_map(|period| period.naive_time().map(|time| (time, period)))
            .filter(|(time, _)| *time > *now)
            .min_by_key(|(time, _)| *time)
            .map(|(_, period)| period)
    }

    pub fn current_status(&self, now: &NaiveTime) -> String {
        let mut passed: Vec<&Period> = self
            .periods
            .iter()
            .filter(|period| period.enabled)
            .filter(|period| {
                period
                    .naive_time()
                    .map(|time| time <= *now)
                    .unwrap_or(false)
            })
            .collect();

        passed
            .pop()
            .map(|period| period.name.clone())
            .unwrap_or_else(|| "待机".to_string())
    }
}

fn default_autostart() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub active_schedule_id: Option<u64>,
    pub next_schedule_id: u64,
    pub schedules: Vec<ScheduleProfile>,
    #[serde(default = "default_autostart")]
    pub autostart: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

impl AppConfig {
    pub fn default_config() -> Self {
        let id = 1;
        Self {
            active_schedule_id: Some(id),
            next_schedule_id: id + 1,
            schedules: vec![ScheduleProfile::default_preset(id)],
            autostart: true,
        }
    }

    pub fn active_schedule(&self) -> Option<&ScheduleProfile> {
        let id = self.active_schedule_id?;
        self.schedules.iter().find(|schedule| schedule.id == id)
    }

    pub fn active_schedule_mut(&mut self) -> Option<&mut ScheduleProfile> {
        let id = self.active_schedule_id?;
        self.schedules.iter_mut().find(|schedule| schedule.id == id)
    }

    pub fn ensure_active_schedule(&mut self) {
        if self.active_schedule_id.is_some() && self.active_schedule().is_some() {
            return;
        }

        self.active_schedule_id = self.schedules.first().map(|schedule| schedule.id);
    }

    pub fn create_empty_schedule(&mut self, name: String) -> u64 {
        let id = self.next_schedule_id;
        self.next_schedule_id += 1;

        self.schedules.push(ScheduleProfile::empty(id, &name));
        self.active_schedule_id = Some(id);
        id
    }

    pub fn remove_active_schedule(&mut self) -> Option<ScheduleProfile> {
        let active_id = self.active_schedule_id?;
        let index = self
            .schedules
            .iter()
            .position(|schedule| schedule.id == active_id)?;

        let removed = self.schedules.remove(index);
        self.active_schedule_id = self.schedules.first().map(|schedule| schedule.id);
        Some(removed)
    }

    pub fn set_active_schedule(&mut self, id: Option<u64>) {
        self.active_schedule_id = id.filter(|candidate| {
            self.schedules
                .iter()
                .any(|schedule| schedule.id == *candidate)
        });

        self.ensure_active_schedule();
    }
}

/// 将用户输入规范化为 HH:MM:SS 格式
/// - 支持输入 "9:5:3" → "09:05:03"
/// - 支持输入 "9:5" → "09:05:00"（补秒）
/// - 如果格式无效返回 None
pub fn normalize_time_str(input: &str) -> Option<String> {
    let parts: Vec<&str> = input.trim().split(':').collect();
    match parts.len() {
        2 => {
            let h: u32 = parts[0].trim().parse().ok()?;
            let m: u32 = parts[1].trim().parse().ok()?;
            if h > 23 || m > 59 {
                return None;
            }
            Some(format!("{:02}:{:02}:00", h, m))
        }
        3 => {
            let h: u32 = parts[0].trim().parse().ok()?;
            let m: u32 = parts[1].trim().parse().ok()?;
            let s: u32 = parts[2].trim().parse().ok()?;
            if h > 23 || m > 59 || s > 59 {
                return None;
            }
            Some(format!("{:02}:{:02}:{:02}", h, m, s))
        }
        _ => None,
    }
}
