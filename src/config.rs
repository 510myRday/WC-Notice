use std::fs;
use std::path::PathBuf;

use crate::schedule::AppConfig;

/// 获取配置文件路径：~/.config/wc_notice/schedule.toml (Linux)
/// 或 %APPDATA%\wc_notice\schedule.toml (Windows)
pub fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("wc_notice").join("schedule.toml")
}

pub fn load_config() -> AppConfig {
    let path = config_path();

    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<AppConfig>(&content) {
                Ok(mut config) => {
                    config.ensure_active_schedule();
                    log::info!("已从 {:?} 加载配置", path);
                    return config;
                }
                Err(e) => log::warn!("配置解析失败，回退默认配置: {}", e),
            },
            Err(e) => log::warn!("配置读取失败，回退默认配置: {}", e),
        }
    }

    let config = AppConfig::default_config();
    let _ = save_config(&config);
    config
}

pub fn save_config(config: &AppConfig) -> anyhow::Result<()> {
    let path = config_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = toml::to_string_pretty(config)?;
    fs::write(&path, content)?;
    log::info!("配置已保存到 {:?}", path);
    Ok(())
}
