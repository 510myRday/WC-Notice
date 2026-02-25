use chrono::{NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

/// 时段类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PeriodType {
    ClassStart,   // 上课
    ClassEnd,     // 下课
    Exercise,     // 课间操
    LunchBreak,   // 午休
    EveningStudy, // 晚自习开始
    EveningEnd,   // 晚自习结束
    Custom,       // 自定义
}

impl PeriodType {
    pub fn label(&self) -> &str {
        match self {
            PeriodType::ClassStart => "上课",
            PeriodType::ClassEnd => "下课",
            PeriodType::Exercise => "课间操",
            PeriodType::LunchBreak => "午休",
            PeriodType::EveningStudy => "晚自习",
            PeriodType::EveningEnd => "晚自习结束",
            PeriodType::Custom => "自定义",
        }
    }

    pub fn bell_type(&self) -> BellType {
        match self {
            PeriodType::ClassStart | PeriodType::EveningStudy => BellType::ClassStart,
            PeriodType::ClassEnd | PeriodType::EveningEnd => BellType::ClassEnd,
            PeriodType::Exercise => BellType::Exercise,
            PeriodType::LunchBreak => BellType::LunchBreak,
            PeriodType::Custom => BellType::ClassStart,
        }
    }
}

/// 铃声类型
#[derive(Debug, Clone, PartialEq)]
pub enum BellType {
    ClassStart,
    ClassEnd,
    Exercise,
    LunchBreak,
}

/// 单个时间节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Period {
    /// HH:MM 格式，如 "08:00"
    pub time: String,
    pub period_type: PeriodType,
    pub name: String,
    pub enabled: bool,
}

impl Period {
    pub fn new(time: &str, period_type: PeriodType, name: &str) -> Self {
        Self {
            time: time.to_string(),
            period_type,
            name: name.to_string(),
            enabled: true,
        }
    }

    /// 解析为 NaiveTime
    pub fn naive_time(&self) -> Option<NaiveTime> {
        NaiveTime::parse_from_str(&self.time, "%H:%M").ok()
    }

    /// 判断当前时间是否命中此节点（精确到分钟）
    pub fn matches_now(&self, now: &NaiveTime) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(t) = self.naive_time() {
            t.hour() == now.hour() && t.minute() == now.minute()
        } else {
            false
        }
    }
}

/// 完整作息时间表
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub name: String,
    pub periods: Vec<Period>,
}

impl Schedule {
    /// 内置默认高中作息时间表
    pub fn default_high_school() -> Self {
        let periods = vec![
            Period::new("08:00", PeriodType::ClassStart, "第1节课"),
            Period::new("08:45", PeriodType::ClassEnd, "第1节课结束"),
            Period::new("08:55", PeriodType::ClassStart, "第2节课"),
            Period::new("09:40", PeriodType::ClassEnd, "第2节课结束"),
            Period::new("09:50", PeriodType::Exercise, "课间操"),
            Period::new("10:10", PeriodType::ClassStart, "第3节课"),
            Period::new("10:55", PeriodType::ClassEnd, "第3节课结束"),
            Period::new("11:05", PeriodType::ClassStart, "第4节课"),
            Period::new("11:50", PeriodType::LunchBreak, "午休开始"),
            Period::new("13:50", PeriodType::ClassStart, "第5节课"),
            Period::new("14:35", PeriodType::ClassEnd, "第5节课结束"),
            Period::new("14:45", PeriodType::ClassStart, "第6节课"),
            Period::new("15:30", PeriodType::ClassEnd, "第6节课结束"),
            Period::new("15:40", PeriodType::ClassStart, "第7节课"),
            Period::new("16:25", PeriodType::ClassEnd, "第7节课结束"),
            Period::new("19:00", PeriodType::EveningStudy, "晚自习开始"),
            Period::new("21:30", PeriodType::EveningEnd, "晚自习结束"),
        ];
        Self {
            name: "高中作息时间表".to_string(),
            periods,
        }
    }

    /// 获取当前时间之后最近的下一个节点
    pub fn next_period(&self, now: &NaiveTime) -> Option<&Period> {
        self.periods
            .iter()
            .filter(|p| p.enabled)
            .filter_map(|p| p.naive_time().map(|t| (t, p)))
            .filter(|(t, _)| *t > *now)
            .min_by_key(|(t, _)| *t)
            .map(|(_, p)| p)
    }

    /// 获取当前所处时段描述
    pub fn current_status(&self, now: &NaiveTime) -> String {
        let passed: Vec<&Period> = self
            .periods
            .iter()
            .filter(|p| p.enabled)
            .filter(|p| p.naive_time().map(|t| t <= *now).unwrap_or(false))
            .collect();

        if let Some(last) = passed.last() {
            last.name.clone()
        } else {
            "课前".to_string()
        }
    }
}
