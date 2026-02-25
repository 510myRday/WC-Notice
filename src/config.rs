use std::fs;
use std::path::PathBuf;

use crate::schedule::Schedule;

/// 获取配置文件路径：~/.config/wc_notice/schedule.toml (Linux)
/// 或 %APPDATA%\wc_notice\schedule.toml (Windows)
pub fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("wc_notice").join("schedule.toml")
}

/// 从文件加载时间表，不存在则返回默认值
pub fn load_schedule() -> Schedule {
    let path = config_path();
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<Schedule>(&content) {
                Ok(schedule) => {
                    log::info!("已从 {:?} 加载时间表", path);
                    return schedule;
                }
                Err(e) => log::warn!("时间表解析失败，使用默认值: {}", e),
            },
            Err(e) => log::warn!("时间表读取失败，使用默认值: {}", e),
        }
    }
    let default = Schedule::default_high_school();
    // 首次运行自动保存默认配置
    let _ = save_schedule(&default);
    default
}

/// 保存时间表到配置文件
pub fn save_schedule(schedule: &Schedule) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(schedule)?;
    fs::write(&path, content)?;
    log::info!("时间表已保存到 {:?}", path);
    Ok(())
}
