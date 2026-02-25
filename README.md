# WC Notice 🔔

程序员专用「学校作息铃声」桌面提醒工具（Rust）。

- 按系统时间触发提醒
- 支持上课/下课/课间操/午休/晚自习等节点
- 桌面通知 + 响铃
- 计划支持 Windows / Linux

## 功能

- 内置默认高中作息（可编辑）
- 每秒检测时间，分钟级触发
- 防重复触发（同一分钟只提醒一次）
- 本地配置持久化（`schedule.toml`）

## 技术栈

- GUI: `egui` + `eframe`
- 时间: `chrono`
- 音频: `rodio`
- 通知: `notify-rust`
- 配置: `serde` + `toml`

## 本地运行

```bash
cargo run
```

## Linux 依赖

在 Ubuntu / Debian 上建议先安装：

```bash
sudo apt update
sudo apt install -y \
  libasound2-dev pkg-config libdbus-1-dev \
  libxkbcommon-dev libwayland-dev libx11-dev
```

> 桌面通知依赖系统通知服务（DBus）。

## 资源文件

当前 `assets/` 下为占位文件，请自行替换：

- `assets/icon.png`
- `assets/bell_start.wav`
- `assets/bell_end.wav`
- `assets/bell_exercise.wav`
- `assets/bell_lunch.wav`

## 配置文件位置

- Linux: `~/.config/wc_notice/schedule.toml`
- Windows: `%APPDATA%\wc_notice\schedule.toml`

## 开源与发布

- License: MIT
- CI: `.github/workflows/ci.yml`
- Release 自动构建: `.github/workflows/release.yml`

### 发布步骤（自动上传 Release 资产）

```bash
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions 会自动构建：

- `wc_notice-x86_64-unknown-linux-gnu.tar.gz`
- `wc_notice-x86_64-pc-windows-msvc.zip`

## 计划

- [ ] 系统托盘（tray）
- [ ] 多时间表模板
- [ ] 铃声自定义
- [ ] i18n（中英文界面）

