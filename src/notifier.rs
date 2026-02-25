use crate::schedule::{BuiltinSound, PeriodKind, SoundSlots, SoundSource};
use rodio::{Decoder, OutputStream, Sink};
use std::fs;
use std::io::Cursor;

static BELL_START: &[u8] = include_bytes!("../assets/bell_start.mp3");
static BELL_END: &[u8] = include_bytes!("../assets/bell_end.mp3");
static BELL_FUN: &[u8] = include_bytes!("../assets/bell_other.mp3");

#[derive(Debug)]
enum PreparedSound {
    Builtin(BuiltinSound),
    Local(Vec<u8>),
}

fn builtin_sound_bytes(sound: BuiltinSound) -> &'static [u8] {
    match sound {
        BuiltinSound::BellStart => BELL_START,
        BuiltinSound::BellEnd => BELL_END,
        BuiltinSound::Fun => BELL_FUN,
    }
}

fn append_sound(sink: &Sink, sound: PreparedSound) -> Result<(), String> {
    let bytes = match sound {
        PreparedSound::Builtin(builtin) => builtin_sound_bytes(builtin).to_vec(),
        PreparedSound::Local(bytes) => bytes,
    };

    let cursor = Cursor::new(bytes);
    let source = Decoder::new(cursor).map_err(|e| e.to_string())?;
    sink.append(source);
    Ok(())
}

/// 播放节点对应音效（在独立线程中播放，不阻塞主线程）。
///
/// 返回值：
/// - Some("本地音效失效，已回退默认")：本次本地音效无效并已自动回退
/// - None：正常使用所选音效
pub fn play_sound_for_period(kind: PeriodKind, slots: &SoundSlots) -> Option<String> {
    let (selected, default_builtin) = match kind {
        PeriodKind::Start => (&slots.start, BuiltinSound::BellStart),
        PeriodKind::End => (&slots.end, BuiltinSound::BellEnd),
    };

    let mut warning: Option<String> = None;
    let mut fallback_on_decode: Option<BuiltinSound> = None;

    let prepared = match selected {
        SoundSource::Builtin(sound) => PreparedSound::Builtin(*sound),
        SoundSource::Local { path } => match fs::read(path) {
            Ok(bytes) => {
                // 在主线程提前做一次解码可用性检查，避免在播放线程才发现本地文件损坏。
                if Decoder::new(Cursor::new(bytes.clone())).is_ok() {
                    fallback_on_decode = Some(default_builtin);
                    PreparedSound::Local(bytes)
                } else {
                    warning = Some("本地音效失效，已回退默认".to_string());
                    PreparedSound::Builtin(default_builtin)
                }
            }
            Err(e) => {
                log::warn!("读取本地音效失败（{}）: {}", path, e);
                warning = Some("本地音效失效，已回退默认".to_string());
                PreparedSound::Builtin(default_builtin)
            }
        },
    };

    std::thread::spawn(move || match OutputStream::try_default() {
        Ok((_stream, handle)) => match Sink::try_new(&handle) {
            Ok(sink) => match append_sound(&sink, prepared) {
                Ok(_) => sink.sleep_until_end(),
                Err(e) => {
                    log::warn!("铃声解码失败: {}", e);
                    if let Some(fallback) = fallback_on_decode {
                        if append_sound(&sink, PreparedSound::Builtin(fallback)).is_ok() {
                            sink.sleep_until_end();
                        } else {
                            log::warn!("回退默认音效也失败");
                        }
                    }
                }
            },
            Err(e) => log::warn!("音频 Sink 初始化失败: {}", e),
        },
        Err(e) => log::warn!("音频输出设备初始化失败: {}", e),
    });

    warning
}

/// 发送系统桌面通知
pub fn send_notification(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();

    std::thread::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        {
            use notify_rust::Notification;

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
