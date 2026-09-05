//! 倒计时到点提示音。
//!
//! 提示音在程序启动后按需生成（三声上行短音 + 停顿，16-bit 单声道 PCM WAV），
//! Windows 上通过系统自带的 winmm `PlaySound` 从内存循环播放，不依赖第三方音频库；
//! 其他平台（开发调试用）落到 `afplay` 命令。

use std::f32::consts::PI;
use std::sync::OnceLock;

const SAMPLE_RATE: u32 = 22_050;

/// 开始循环播放提示音（重复调用会重新开始）
pub fn start() {
    platform::start();
}

/// 停止提示音（未在播放时调用无副作用）
pub fn stop() {
    platform::stop();
}

fn wav_bytes() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(build_wav)
}

/// 生成一个循环周期的提示音：3 声上行短音（A5 → C#6 → E6）+ 约 0.9s 停顿
fn build_wav() -> Vec<u8> {
    const BEEPS: [(f32, f32); 3] = [(880.0, 0.12), (1108.7, 0.12), (1318.5, 0.18)]; // (频率 Hz, 时长 s)
    const GAP: f32 = 0.08; // 短音之间的间隔 s
    const TAIL: f32 = 0.9; // 一轮结束后的停顿 s
    const AMPLITUDE: f32 = 0.45; // 音量（0~1）
    const FADE: f32 = 0.006; // 每个短音的淡入淡出 s，避免爆音

    let secs_to_samples = |secs: f32| (secs * SAMPLE_RATE as f32).round() as usize;
    let mut samples: Vec<i16> = Vec::new();

    for (i, (freq, dur)) in BEEPS.iter().enumerate() {
        let n = secs_to_samples(*dur);
        let fade_n = secs_to_samples(FADE).max(1);
        for k in 0..n {
            let t = k as f32 / SAMPLE_RATE as f32;
            let envelope = if k < fade_n {
                k as f32 / fade_n as f32
            } else if n - k <= fade_n {
                (n - k) as f32 / fade_n as f32
            } else {
                1.0
            };
            let v = (2.0 * PI * freq * t).sin() * AMPLITUDE * envelope;
            samples.push((v * i16::MAX as f32) as i16);
        }
        if i + 1 < BEEPS.len() {
            samples.extend(std::iter::repeat(0).take(secs_to_samples(GAP)));
        }
    }
    samples.extend(std::iter::repeat(0).take(secs_to_samples(TAIL)));

    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt 块长度
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // 单声道
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // 字节率
    wav.extend_from_slice(&2u16.to_le_bytes()); // 块对齐
    wav.extend_from_slice(&16u16.to_le_bytes()); // 位深
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::ptr;

    const SND_ASYNC: u32 = 0x0001; // 异步播放，立即返回
    const SND_NODEFAULT: u32 = 0x0002; // 找不到声音时不播放系统默认音
    const SND_MEMORY: u32 = 0x0004; // 第一个参数指向内存中的 WAV 数据
    const SND_LOOP: u32 = 0x0008; // 循环播放直到再次调用 PlaySound

    #[link(name = "winmm")]
    extern "system" {
        fn PlaySoundW(pszSound: *const u16, hmod: *mut c_void, fdwSound: u32) -> i32;
    }

    pub fn start() {
        let wav = super::wav_bytes(); // 'static，播放期间内存一直有效
        unsafe {
            PlaySoundW(
                wav.as_ptr() as *const u16,
                ptr::null_mut(),
                SND_MEMORY | SND_ASYNC | SND_LOOP | SND_NODEFAULT,
            );
        }
    }

    pub fn stop() {
        unsafe {
            PlaySoundW(ptr::null(), ptr::null_mut(), 0);
        }
    }
}

/// 非 Windows 平台（仅用于 macOS 上开发调试）：把 WAV 写到临时目录，用 afplay 循环播放
#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    static GENERATION: AtomicU64 = AtomicU64::new(0);
    static CHILD: Mutex<Option<Child>> = Mutex::new(None);

    fn wav_path() -> PathBuf {
        std::env::temp_dir().join("jz_alarm.wav")
    }

    pub fn start() {
        stop();
        let path = wav_path();
        if std::fs::write(&path, super::wav_bytes()).is_err() {
            return;
        }
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::spawn(move || {
            while GENERATION.load(Ordering::SeqCst) == generation {
                let spawned = Command::new("afplay")
                    .arg(&path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                let Ok(child) = spawned else { return };
                *CHILD.lock().unwrap() = Some(child);
                // 播放期间不长期持锁，以便 stop() 能随时 kill
                loop {
                    std::thread::sleep(Duration::from_millis(50));
                    let mut guard = CHILD.lock().unwrap();
                    match guard.as_mut().map(|c| c.try_wait()) {
                        Some(Ok(None)) => {}
                        _ => {
                            *guard = None;
                            break;
                        }
                    }
                }
            }
        });
    }

    pub fn stop() {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        if let Some(mut child) = CHILD.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn wav_is_well_formed() {
        let wav = super::build_wav();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        let riff_len = u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize;
        let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(riff_len, wav.len() - 8);
        assert_eq!(data_len, wav.len() - 44);
        // 一轮约 1.5s：3 段短音 + 2 段间隔 + 尾部停顿
        let secs = data_len as f32 / 2.0 / super::SAMPLE_RATE as f32;
        assert!((1.4..1.6).contains(&secs), "unexpected loop length {secs}s");
    }
}
