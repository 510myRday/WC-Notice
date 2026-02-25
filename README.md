# WC Notice

轻量的桌面作息提醒工具（Rust + egui），支持多时间表、桌面通知、铃声与系统托盘。

## 特性

- 按系统时间触发提醒（后台每秒检查，按分钟命中）
- 桌面通知 + 音效播放
- 多时间表管理：新建、切换、重命名、删除
- 节点管理：`开始` / `结束` 两类节点，可启停、排序、编辑、删除
- 每个时间表独立音效槽位：`开始音效`、`结束音效`
- 音效来源可选：内置音效或本地文件（`mp3` / `wav`）
- 本地音效读取/解码失败时自动回退默认内置音效
- 支持系统托盘：最小化到托盘、托盘恢复窗口、托盘菜单退出
- 关闭窗口时二次确认（可选择“最小化到托盘”或“退出程序”）
- 配置自动持久化（防抖写盘）

## 运行

```bash
cargo run
```

## 使用说明

- 顶部栏可查看当前状态、下一节点倒计时，并进行暂停/恢复提醒
- `📋`：切换或重命名当前时间表
- `➕`：新建空时间表
- `🔔`：配置当前时间表的开始/结束音效
- 主区域 `+`：添加节点（时间格式 `HH:MM`）
- 关闭窗口时可选择最小化到托盘，提醒会继续运行

## 资源文件（必须存在）

- `assets/icon.png`
- `assets/bell_start.mp3`
- `assets/bell_end.mp3`
- `assets/bell_other.mp3`

## 配置文件

默认保存为单文件 `schedule.toml`：

- Windows: `%APPDATA%\wc_notice\schedule.toml`
- macOS: `~/Library/Application Support/wc_notice/schedule.toml`
- Linux: `~/.config/wc_notice/schedule.toml`

配置顶层结构：

- `active_schedule_id: Option<u64>`
- `next_schedule_id: u64`
- `schedules: Vec<ScheduleProfile>`

`ScheduleProfile` 包含：

- `id`
- `name`
- `periods`（每个节点：`time` / `kind(Start|End)` / `name` / `enabled`）
- `sound`（`start` / `end`，支持 `Builtin(BellStart|BellEnd|Fun)` 或 `Local { path }`）

## 平台支持与依赖

支持 Windows / macOS / Linux（含托盘功能）。托盘初始化失败时，程序会继续运行（仅不启用托盘）。

Windows 与 macOS 一般无需额外依赖；Linux（Ubuntu / Debian）建议先安装：

```bash
sudo apt update
sudo apt install -y \
  libasound2-dev pkg-config libdbus-1-dev \
  libxkbcommon-dev libwayland-dev libx11-dev \
  libgtk-3-dev libglib2.0-dev libappindicator3-dev \
  libxrandr-dev libxi-dev libxcursor-dev
```

## 开源信息

- License: MIT
- CI: `.github/workflows/ci.yml`
- Release: `.github/workflows/release.yml`
