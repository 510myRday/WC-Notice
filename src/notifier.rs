use crate::schedule::BellType;
use rodio::{Decoder, OutputStream, Sink};
use std::io::Cursor;

// 内嵌铃声资源
static BELL_CLASS_START: &[u8] = include_bytes!("../assets/bell_start.wav");
static BELL_CLASS_END: &[u8] = include_bytes!("../assets/bell_end.wav");
static BELL_EXERCISE: &[u8] = include_bytes!("../assets/bell_exercise.wav");
static BELL_LUNCH: &[u8] = include_bytes!("../assets/bell_lunch.wav");

/// 播放对应铃声（在单独线程中，不阻塞主线程）
pub fn play_bell(bell_type: &BellType) {
    let data: &'static [u8] = match bell_type {
        BellType::ClassStart => BELL_CLASS_START,
        BellType::ClassEnd => BELL_CLASS_END,
        BellType::Exercise => BELL_EXERCISE,
        BellType::LunchBreak => BELL_LUNCH,
    };

    std::thread::spawn(move || match OutputStream::try_default() {
        Ok((_stream, handle)) => {
            let sink = Sink::try_new(&handle).unwrap();
            let cursor = Cursor::new(data);
            match Decoder::new(cursor) {
                Ok(source) => {
                    sink.append(source);
                    sink.sleep_until_end();
                }
                Err(e) => log::warn!("铃声解码失败: {}", e),
            }
        }
        Err(e) => log::warn!("音频输出设备初始化失败: {}", e),
    });
}

/// 发送系统桌面通知
pub fn send_notification(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        {
            use notify_rust::Notification;
            // macOS 不需要 icon() 调用，否则某些版本会报错
            #[cfg(target_os = "macos")]
            let result = Notification::new()
                .summary(&title)
                .body(&body)
                .timeout(notify_rust::Timeout::Milliseconds(5000))
                .show();

            #[cfg(not(target_os = "macos"))]
            let result = Notification::new()
                .summary(&title)
                .body(&body)
                .icon("dialog-information")
                .timeout(notify_rust::Timeout::Milliseconds(5000))
                .show();

            if let Err(e) = result {
                log::warn!("系统通知发送失败: {}", e);
            }
        }
    });
}
