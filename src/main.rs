#![windows_subsystem = "windows"]

mod alarm;
mod db;

use chrono::{Local, NaiveDate, Datelike};
use db::{Database, Record};
use eframe::egui::{self, Color32, CornerRadius, FontId, RichText, Vec2, Stroke};
use std::sync::Arc;
use std::fs::File;
use std::path::PathBuf;
use std::time::{Duration, Instant};

// ===== UI 主题配置 =====
struct Theme {
    bg_color: Color32,
    card_color: Color32,
    input_bg: Color32,
    accent_color: Color32,
    green_color: Color32,
    text_primary: Color32,
    text_secondary: Color32,
    danger_color: Color32,
    warning_color: Color32,
    disabled_text: Color32,
    disabled_bg: Color32,
    border_color: Color32,   // 输入框/按钮描边
    accent_dim: Color32,     // 不可交互时的选中态强调色
}

impl Theme {
    fn default() -> Self {
        Self {
            bg_color: Color32::from_rgb(25, 28, 32),
            card_color: Color32::from_rgb(35, 39, 45),
            input_bg: Color32::from_rgb(45, 50, 58),
            accent_color: Color32::from_rgb(64, 169, 255),
            green_color: Color32::from_rgb(82, 196, 126),
            text_primary: Color32::from_rgb(230, 230, 235),
            text_secondary: Color32::from_rgb(140, 145, 155),
            danger_color: Color32::from_rgb(220, 80, 80),
            warning_color: Color32::from_rgb(230, 180, 80),
            disabled_text: Color32::from_rgb(80, 85, 95),
            disabled_bg: Color32::from_rgb(45, 48, 55),
            border_color: Color32::from_rgb(60, 65, 75),
            accent_dim: Color32::from_rgb(40, 70, 100),
        }
    }
}

// ===== 布局常量配置 =====
struct LayoutConfig {
    content_width: f32,
    panel_margin: f32,
    card_rounding: f32,
    card_inner_margin: f32,
    col_spacing: f32,
}

impl LayoutConfig {
    fn default() -> Self {
        Self {
            content_width: 880.0,
            panel_margin: 32.0,
            card_rounding: 14.0,
            card_inner_margin: 22.0,
            col_spacing: 10.0,
        }
    }
}

fn get_lock_file_path() -> PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("jz");
    std::fs::create_dir_all(&path).ok();
    path.push(".lock");
    path
}

fn try_lock() -> Option<File> {
    let lock_path = get_lock_file_path();

    // 尝试以独占方式打开文件
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lock_path)
        .ok()?;

    // Windows 上使用文件锁
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;

        #[link(name = "kernel32")]
        extern "system" {
            fn LockFile(
                hFile: *mut std::ffi::c_void,
                dwFileOffsetLow: u32,
                dwFileOffsetHigh: u32,
                nNumberOfBytesToLockLow: u32,
                nNumberOfBytesToLockHigh: u32,
            ) -> i32;
        }

        unsafe {
            let handle = file.as_raw_handle();
            if LockFile(handle as *mut _, 0, 0, 1, 0) == 0 {
                return None;
            }
        }
    }

    Some(file)
}

fn main() -> eframe::Result<()> {
    // 确保只运行一个实例
    let _lock = try_lock();
    if _lock.is_none() {
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 810.0])
            .with_min_inner_size([960.0, 810.0]),
        ..Default::default()
    };
    eframe::run_native(
        "记账本",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 系统中文字体：Windows 用微软雅黑；macOS 上开发调试时用黑体（同样注册为 "msyh"，供各字体族回退）
    #[cfg(target_os = "macos")]
    let system_font_path = "/System/Library/Fonts/STHeiti Light.ttc";
    #[cfg(not(target_os = "macos"))]
    let system_font_path = "C:\\Windows\\Fonts\\msyh.ttc";

    if let Ok(font_data) = std::fs::read(system_font_path) {
        fonts.font_data.insert(
            "msyh".to_owned(),
            Arc::new(egui::FontData::from_owned(font_data)),
        );
        fonts.families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "msyh".to_owned());
        fonts.families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .insert(0, "msyh".to_owned());
    }

    // 内嵌站酷快乐体用于标题
    fonts.font_data.insert(
        "zcool_kuaile".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!("../fonts/ZCOOLKuaiLe-Regular.ttf"))),
    );
    fonts.families.insert(
        egui::FontFamily::Name("cute".into()),
        vec!["zcool_kuaile".to_owned(), "msyh".to_owned()],
    );

    ctx.set_fonts(fonts);
}

/// 计时栏模式
#[derive(Clone, Copy, PartialEq, Eq)]
enum TimerMode {
    Stopwatch, // 正计时
    Countdown, // 倒计时
}

/// 倒计时到点后提示音最长持续秒数，超时自动静音（"时间到"显示保留到手动重置）
const ALARM_SOUND_MAX_SECS: u64 = 60;

/// 秒数格式化为 HH:MM:SS
fn format_hms(total_secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", total_secs / 3600, (total_secs % 3600) / 60, total_secs % 60)
}

struct App {
    db: Database,
    records: Vec<Record>,
    total_balance: f64,
    day_balance: f64,
    month_balance: f64,
    boss_balances: std::collections::HashMap<String, f64>,
    boss_list: Vec<String>,
    game_list: Vec<String>,

    // 月结余选择器
    selected_year: i32,
    selected_month: u32,

    input_date: NaiveDate,
    input_boss: String,
    input_income: String,
    input_duration: String,      // 时长输入
    input_game: String,          // 游戏输入
    input_settled: bool,         // 是否结清勾选
    show_boss_suggestions: bool,
    show_game_suggestions: bool, // 游戏联想显示

    // 输入验证错误状态
    input_boss_error: bool,
    input_income_error: bool,

    message: String,
    message_is_error: bool,
    message_timer: f32,

    // 计时器
    timer_running: bool,
    timer_start_instant: Option<Instant>,
    timer_accumulated: Duration,
    timer_ended: bool,  // 是否已结束（结束后才能重置）

    // 倒计时与到点提醒
    timer_mode: TimerMode,          // 正计时 / 倒计时
    countdown_hours: u32,           // 倒计时设置：小时
    countdown_minutes: u32,         // 倒计时设置：分钟
    alarm_active: bool,             // 倒计时已到点、尚未重置
    alarm_sound_playing: bool,      // 提示音是否正在循环播放
    alarm_started: Option<Instant>, // 到点时刻（用于闪烁与自动静音）
}

impl App {
    fn new() -> Self {
        let db = Database::new().expect("无法初始化数据库");
        let records = db.get_all_records().unwrap_or_default();
        let today = Local::now().date_naive();
        let total_balance = Self::calc_year_balance(&records, today.year());
        let day_balance = Self::calc_day_balance(&records, &today.format("%Y-%m-%d").to_string());
        let month_balance = Self::calc_month_balance(&records, &today.format("%Y-%m").to_string());
        let boss_balances = Self::calc_boss_balances(&records);

        let boss_list = db.get_all_bosses();
        let game_list = db.get_all_games();

        Self {
            db,
            records,
            total_balance,
            day_balance,
            month_balance,
            boss_balances,
            boss_list,
            game_list,
            selected_year: today.year(),
            selected_month: today.month(),
            input_date: today,
            input_boss: String::new(),
            input_income: String::new(),
            input_duration: String::new(),
            input_game: String::new(),
            input_settled: false,
            show_boss_suggestions: false,
            show_game_suggestions: false,
            input_boss_error: false,
            input_income_error: false,
            message: String::new(),
            message_is_error: false,
            message_timer: 0.0,
            timer_running: false,
            timer_start_instant: None,
            timer_accumulated: Duration::ZERO,
            timer_ended: false,
            timer_mode: TimerMode::Stopwatch,
            countdown_hours: 1,
            countdown_minutes: 0,
            alarm_active: false,
            alarm_sound_playing: false,
            alarm_started: None,
        }
    }

    fn calc_boss_balances(records: &[Record]) -> std::collections::HashMap<String, f64> {
        let mut map = std::collections::HashMap::new();
        for r in records {
            *map.entry(r.boss.clone()).or_insert(0.0) += r.income;
        }
        map
    }

    fn calc_day_balance(records: &[Record], date: &str) -> f64 {
        records.iter()
            .filter(|r| r.date == date)
            .map(|r| r.income)
            .sum()
    }

    fn calc_year_balance(records: &[Record], year: i32) -> f64 {
        let year_str = format!("{}", year);
        records.iter()
            .filter(|r| r.date.starts_with(&year_str))
            .map(|r| r.income)
            .sum()
    }

    fn calc_month_balance(records: &[Record], year_month: &str) -> f64 {
        records.iter()
            .filter(|r| r.date.starts_with(year_month))
            .map(|r| r.income)
            .sum()
    }

    fn refresh_data(&mut self) {
        self.records = self.db.get_all_records().unwrap_or_default();
        self.total_balance = Self::calc_year_balance(&self.records, self.selected_year);
        self.day_balance = Self::calc_day_balance(&self.records, &self.input_date.format("%Y-%m-%d").to_string());
        let year_month = format!("{}-{:02}", self.selected_year, self.selected_month);
        self.month_balance = Self::calc_month_balance(&self.records, &year_month);
        self.boss_balances = Self::calc_boss_balances(&self.records);
        self.boss_list = self.db.get_all_bosses();
        self.game_list = self.db.get_all_games();
    }

    fn show_message(&mut self, msg: &str, is_error: bool) {
        self.message = msg.to_string();
        self.message_is_error = is_error;
        self.message_timer = 3.0;
    }

    fn add_record(&mut self) {
        const MAX_INCOME: f64 = 100_000.0; // 单笔最大10万

        // 重置错误状态
        self.input_boss_error = false;
        self.input_income_error = false;

        // 验证必填项
        let boss_empty = self.input_boss.trim().is_empty();
        let income_invalid = self.input_income.trim().parse::<f64>()
            .map(|v| v <= 0.0 || !v.is_finite())
            .unwrap_or(true);

        if boss_empty || income_invalid {
            if boss_empty {
                self.input_boss_error = true;
            }
            if income_invalid {
                self.input_income_error = true;
            }
            // 显示具体的错误提示
            let msg = match (boss_empty, income_invalid) {
                (true, true) => "请输入老板名称和收入金额",
                (true, false) => "请输入老板名称",
                (false, true) => "请输入有效金额",
                _ => unreachable!(),
            };
            self.show_message(msg, true);
            return;
        }

        let income: f64 = self.input_income.trim().parse().unwrap();

        // 检查单笔金额上限
        if income > MAX_INCOME {
            self.input_income_error = true;
            self.show_message(&format!("单笔金额不能超过 ¥{:.0}", MAX_INCOME), true);
            return;
        }

        // 解析时长（可为空，支持小数）
        let duration: Option<f64> = if self.input_duration.trim().is_empty() {
            None
        } else {
            match self.input_duration.trim().parse::<f64>() {
                Ok(v) if v > 0.0 && v.is_finite() => Some((v * 10.0).round() / 10.0), // 保留一位小数
                _ => {
                    self.show_message("请输入有效时长", true);
                    return;
                }
            }
        };

        // 游戏名称（可为空）
        let game: Option<&str> = if self.input_game.trim().is_empty() {
            None
        } else {
            Some(self.input_game.trim())
        };

        let date_str = self.input_date.format("%Y-%m-%d").to_string();
        match self.db.add_record(&date_str, self.input_boss.trim(), income, duration, game, self.input_settled) {
            Ok(_) => {
                self.show_message(&format!("已添加 ¥{:.2}", income), false);
                self.input_boss.clear();
                self.input_income.clear();
                self.input_duration.clear();
                self.input_game.clear();
                self.input_settled = false;
                self.input_boss_error = false;
                self.input_income_error = false;
                self.refresh_data();
            }
            Err(_) => {
                self.show_message("添加失败", true);
            }
        }
    }

    fn delete_record(&mut self, id: i64) {
        if self.db.delete_record(id).is_ok() {
            self.show_message("已删除", false);
            self.refresh_data();
        }
    }

    // ===== 计时器 / 倒计时 =====

    /// 已走过的时间（正计时=显示值；倒计时=已消耗时长）
    fn timer_elapsed(&self) -> Duration {
        match (self.timer_running, self.timer_start_instant) {
            (true, Some(start)) => self.timer_accumulated + start.elapsed(),
            _ => self.timer_accumulated,
        }
    }

    /// 倒计时设置的总时长
    fn countdown_total(&self) -> Duration {
        Duration::from_secs(u64::from(self.countdown_hours) * 3600 + u64::from(self.countdown_minutes) * 60)
    }

    /// 开始 / 继续
    fn start_timer(&mut self) {
        self.timer_running = true;
        self.timer_start_instant = Some(Instant::now());
        self.timer_ended = false;
    }

    fn pause_timer(&mut self) {
        if let Some(start) = self.timer_start_instant.take() {
            self.timer_accumulated += start.elapsed();
        }
        self.timer_running = false;
    }

    /// 结束：停止计时但保留显示值，之后才能重置
    fn end_timer(&mut self) {
        self.pause_timer();
        self.timer_ended = true;
    }

    fn reset_timer(&mut self, ctx: &egui::Context) {
        self.dismiss_alarm(ctx);
        self.timer_running = false;
        self.timer_start_instant = None;
        self.timer_accumulated = Duration::ZERO;
        self.timer_ended = false;
    }

    /// 倒计时到点：停表 + 循环提示音 + 任务栏闪烁 + 计时栏红色闪烁
    fn trigger_alarm(&mut self, ctx: &egui::Context) {
        self.timer_running = false;
        self.timer_start_instant = None;
        self.timer_accumulated = self.countdown_total();
        self.timer_ended = true;
        self.alarm_active = true;
        self.alarm_sound_playing = true;
        self.alarm_started = Some(Instant::now());
        alarm::start();
        // Windows 上会持续闪烁任务栏，直到窗口回到前台
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Critical));
    }

    /// 静音：停止提示音，保留"时间到"显示
    fn stop_alarm_sound(&mut self) {
        if self.alarm_sound_playing {
            alarm::stop();
            self.alarm_sound_playing = false;
        }
    }

    /// 完全解除提醒（重置时调用）
    fn dismiss_alarm(&mut self, ctx: &egui::Context) {
        self.stop_alarm_sound();
        if self.alarm_active {
            self.alarm_active = false;
            self.alarm_started = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Reset));
        }
    }

    /// 每帧调用：检测倒计时到点、超时自动静音、安排重绘
    fn tick_timer(&mut self, ctx: &egui::Context) {
        if self.timer_running {
            if self.timer_mode == TimerMode::Countdown && self.timer_elapsed() >= self.countdown_total() {
                self.trigger_alarm(ctx);
            }
            ctx.request_repaint();
        }
        if self.alarm_active {
            let sound_timed_out = self
                .alarm_started
                .map_or(false, |t| t.elapsed().as_secs() >= ALARM_SOUND_MAX_SECS);
            if self.alarm_sound_playing && sound_timed_out {
                self.stop_alarm_sound();
            }
            // 驱动"时间到"闪烁动画
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    /// 计时栏左半部分：模式切换 + 时间显示 + 控制按钮
    fn timer_controls_ui(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let is_countdown = self.timer_mode == TimerMode::Countdown;
        let is_initial = !self.timer_running && self.timer_accumulated.is_zero() && !self.timer_ended;
        let is_running = self.timer_running;
        let is_paused = !self.timer_running && !self.timer_accumulated.is_zero() && !self.timer_ended;
        let is_ended = self.timer_ended;
        let elapsed = self.timer_elapsed();
        let countdown_total = self.countdown_total();

        // 各区域固定宽度 + 显式间距，保证状态切换时按钮位置不跳动，且给右侧统计留足空间
        ui.spacing_mut().item_spacing.x = 0.0;
        let mode_slot = Vec2::new(96.0, 30.0);
        let time_slot = Vec2::new(156.0, 40.0);
        let btn_size = [56.0, 30.0];
        let btn_gap = 18.0;

        // ---- 模式切换（仅空闲时可切换）；到点后此处显示"时间到" ----
        ui.allocate_ui_with_layout(mode_slot, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(mode_slot);
            if self.alarm_active {
                ui.label(RichText::new("时间到！").size(16.0).color(theme.danger_color));
                return;
            }
            ui.spacing_mut().item_spacing.x = 4.0;
            for (mode, text) in [(TimerMode::Stopwatch, "正计时"), (TimerMode::Countdown, "倒计时")] {
                let selected = self.timer_mode == mode;
                let (fill, text_color, stroke) = match (selected, is_initial) {
                    (true, true) => (theme.accent_color, Color32::WHITE, Stroke::NONE),
                    (true, false) => (theme.accent_dim, theme.text_primary, Stroke::NONE),
                    (false, true) => (Color32::TRANSPARENT, theme.text_secondary, Stroke::new(1.0, theme.border_color)),
                    (false, false) => (theme.disabled_bg, theme.disabled_text, Stroke::NONE),
                };
                let btn = egui::Button::new(RichText::new(text).size(12.0).color(text_color))
                    .fill(fill)
                    .stroke(stroke)
                    .corner_radius(CornerRadius::same(6));
                if ui.add_sized([46.0, 24.0], btn).clicked() && is_initial {
                    self.timer_mode = mode;
                }
            }
        });

        ui.add_space(12.0);

        // ---- 时间显示；倒计时空闲时显示时/分设置 ----
        ui.allocate_ui_with_layout(time_slot, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(time_slot);
            if is_countdown && is_initial {
                let dark_text = Color32::from_rgb(30, 30, 35);
                ui.spacing_mut().item_spacing.x = 4.0;
                egui::ComboBox::from_id_salt("countdown_hours")
                    .width(56.0)
                    .selected_text(RichText::new(format!("{:02}", self.countdown_hours)).size(13.0).color(dark_text))
                    .show_ui(ui, |ui| {
                        for h in 0..=12u32 {
                            ui.selectable_value(&mut self.countdown_hours, h, format!("{:02}", h));
                        }
                    });
                ui.label(RichText::new("时").size(13.0).color(theme.text_secondary));
                egui::ComboBox::from_id_salt("countdown_minutes")
                    .width(56.0)
                    .selected_text(RichText::new(format!("{:02}", self.countdown_minutes)).size(13.0).color(dark_text))
                    .show_ui(ui, |ui| {
                        // 0~5 逐分钟，之后每 5 分钟一档
                        for m in (0..=5u32).chain((10..60u32).step_by(5)) {
                            ui.selectable_value(&mut self.countdown_minutes, m, format!("{:02}", m));
                        }
                    });
                ui.label(RichText::new("分").size(13.0).color(theme.text_secondary));
            } else {
                let secs = if is_countdown {
                    // 剩余时间向上取整，避免提前显示 00:00:00
                    countdown_total.saturating_sub(elapsed).as_secs_f64().ceil() as u64
                } else {
                    elapsed.as_secs()
                };
                let time_color = if self.alarm_active {
                    // 红色闪烁（0.5s 周期）
                    let on = self.alarm_started.map_or(true, |t| (t.elapsed().as_millis() / 500) % 2 == 0);
                    if on { theme.danger_color } else { Color32::from_rgb(120, 50, 50) }
                } else if is_running {
                    theme.accent_color
                } else if is_paused {
                    theme.warning_color
                } else if is_ended {
                    theme.text_primary
                } else {
                    theme.text_secondary
                };
                ui.label(RichText::new(format_hms(secs)).font(FontId::monospace(32.0)).color(time_color));
            }
        });

        ui.add_space(16.0);

        // ---- 按钮组：开始 / 暂停·继续 / 结束·静音 / 重置 ----
        let ctx = ui.ctx().clone();
        let solid = |text: &str, fill: Color32, color: Color32| {
            egui::Button::new(RichText::new(text).size(13.0).color(color))
                .fill(fill)
                .corner_radius(CornerRadius::same(6))
        };
        let outlined = |text: &str, color: Color32, stroke: Color32| {
            egui::Button::new(RichText::new(text).size(13.0).color(color))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, stroke))
                .corner_radius(CornerRadius::same(6))
        };
        let disabled = |text: &str| solid(text, theme.disabled_bg, theme.disabled_text);
        let disabled_outlined = |text: &str| outlined(text, theme.disabled_text, theme.border_color);

        // 开始（空闲时可用；倒计时需先设置时长）
        let can_start = is_initial && (!is_countdown || !countdown_total.is_zero());
        if can_start {
            if ui.add_sized(btn_size, solid("开始", theme.green_color, Color32::WHITE)).clicked() {
                self.start_timer();
            }
        } else if ui.add_sized(btn_size, disabled("开始")).clicked() && is_initial && is_countdown {
            self.show_message("请先设置倒计时时长", true);
        }

        ui.add_space(btn_gap);

        // 暂停 / 继续
        if is_running {
            if ui.add_sized(btn_size, solid("暂停", theme.warning_color, Color32::WHITE)).clicked() {
                self.pause_timer();
            }
        } else if is_paused {
            if ui.add_sized(btn_size, solid("继续", theme.accent_color, Color32::WHITE)).clicked() {
                self.start_timer();
            }
        } else {
            ui.add_sized(btn_size, disabled("暂停"));
        }

        ui.add_space(btn_gap);

        // 结束；到点提示音播放中时此位置变为"静音"
        if self.alarm_sound_playing {
            if ui.add_sized(btn_size, solid("静音", theme.danger_color, Color32::WHITE)).clicked() {
                self.stop_alarm_sound();
            }
        } else if is_running || is_paused {
            if ui.add_sized(btn_size, outlined("结束", theme.danger_color, theme.danger_color)).clicked() {
                self.end_timer();
            }
        } else {
            ui.add_sized(btn_size, disabled_outlined("结束"));
        }

        ui.add_space(btn_gap);

        // 重置（结束或到点后可用）
        if is_ended {
            if ui.add_sized(btn_size, solid("重置", theme.input_bg, theme.text_secondary)).clicked() {
                self.reset_timer(&ctx);
            }
        } else {
            ui.add_sized(btn_size, disabled("重置"));
        }
    }

    /// 计时栏右侧：所选日期的收入 / 单数 / 时长统计
    fn day_stats_ui(&self, ui: &mut egui::Ui, theme: &Theme) {
        let selected_date_str = self.input_date.format("%Y-%m-%d").to_string();
        let day_records: Vec<&Record> = self.records.iter().filter(|r| r.date == selected_date_str).collect();
        let day_count = day_records.len();
        // 用 fold 而非 sum：空集合的 f64 sum 会得到 -0.0，显示成 "-0.0h"
        let day_hours: f64 = day_records.iter().filter_map(|r| r.duration).fold(0.0, |acc, h| acc + h);
        let day_income: f64 = day_records.iter().map(|r| r.income).sum();
        let is_today = self.input_date == Local::now().date_naive();
        let day_label = if is_today {
            "今日收入".to_string()
        } else {
            format!("{}月{}日", self.input_date.month(), self.input_date.day())
        };

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // right_to_left 布局从右到左添加元素，所以顺序要反过来
            ui.spacing_mut().item_spacing.x = 8.0;
            let text = |s: String| RichText::new(s).size(14.0).color(theme.text_primary);
            ui.label(text(format!("{:.1}h", day_hours)));
            ui.label(text("·".to_string()));
            ui.label(text(format!("{}单", day_count)));
            ui.label(text("·".to_string()));
            ui.label(text(format_money(day_income)));
            ui.add_space(8.0);
            ui.label(text(day_label));
        });
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// 格式化金额显示，大金额使用万/亿为单位
fn format_money(amount: f64) -> String {
    let abs_amount = amount.abs();
    let sign = if amount < 0.0 { "-" } else { "" };

    if abs_amount >= 100_000_000.0 {
        // 亿
        format!("{}¥{:.2}亿", sign, abs_amount / 100_000_000.0)
    } else if abs_amount >= 100_000.0 {
        // 万
        format!("{}¥{:.2}万", sign, abs_amount / 10_000.0)
    } else {
        format!("{}¥{:.2}", sign, abs_amount)
    }
}

/// 格式化收入显示（带+号）
fn format_income(amount: f64) -> String {
    let abs_amount = amount.abs();

    if abs_amount >= 100_000_000.0 {
        format!("+{:.2}亿", abs_amount / 100_000_000.0)
    } else if abs_amount >= 100_000.0 {
        format!("+{:.2}万", abs_amount / 10_000.0)
    } else {
        format!("+{:.2}", abs_amount)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 消息计时器
        if self.message_timer > 0.0 {
            self.message_timer -= ctx.input(|i| i.unstable_dt);
            if self.message_timer <= 0.0 {
                self.message.clear();
            }
            ctx.request_repaint();
        }

        // 计时器 / 倒计时推进（含到点提醒）
        self.tick_timer(ctx);

        // 加载主题和布局配置
        let theme = Theme::default();
        let layout = LayoutConfig::default();

        // 解构主题颜色以保持代码兼容性
        let bg_color = theme.bg_color;
        let card_color = theme.card_color;
        let input_bg = theme.input_bg;
        let accent_color = theme.accent_color;
        let green_color = theme.green_color;
        let text_primary = theme.text_primary;
        let text_secondary = theme.text_secondary;
        let danger_color = theme.danger_color;

        // ===== 底部计时器栏（固定在底部）=====
        let timer_border = if self.alarm_active { danger_color } else { accent_color };
        egui::TopBottomPanel::bottom("timer_panel")
            .frame(egui::Frame::default().fill(bg_color).inner_margin(egui::Margin {
                left: layout.panel_margin as i8,
                right: layout.panel_margin as i8,
                top: 8,
                bottom: 16
            }))
            .show(ctx, |ui| {
                // 与内容区域等宽居中
                let content_width = layout.content_width;
                let available = ui.available_width();
                let side_margin = ((available - content_width) / 2.0).max(0.0);

                ui.horizontal(|ui| {
                    ui.add_space(side_margin);
                    ui.vertical(|ui| {
                        ui.set_width(content_width);
                        egui::Frame::default()
                            .fill(Color32::TRANSPARENT)  // 透明背景
                            .stroke(Stroke::new(1.0, timer_border))  // 细边框，到点时变红
                            .corner_radius(CornerRadius::same(layout.card_rounding as u8))
                            .inner_margin(egui::Margin::symmetric(layout.card_inner_margin as i8, 16))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    self.timer_controls_ui(ui, &theme);
                                    self.day_stats_ui(ui, &theme);
                                });
                            });
                    });
                });
            });

        // 设置全局样式
        let mut style = (*ctx.style()).clone();
        style.visuals.widgets.inactive.bg_fill = input_bg;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(60, 65, 75));
        style.visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(55, 60, 70);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(50, 55, 65);
        style.visuals.selection.bg_fill = accent_color;
        ctx.set_style(style);

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(bg_color).inner_margin(layout.panel_margin as i8))
            .show(ctx, |ui| {
                // 固定内容宽度，居中显示
                let content_width = layout.content_width;
                let available = ui.available_width();
                let side_margin = ((available - content_width) / 2.0).max(0.0);

                ui.horizontal(|ui| {
                    ui.add_space(side_margin);
                    ui.vertical(|ui| {
                        ui.set_width(content_width);

                // ===== 顶部标题区 =====
                let mut month_changed = false;
                let mut new_sel_year = self.selected_year;
                let mut new_sel_month = self.selected_month;
                let combo_text_color = Color32::from_rgb(30, 30, 35); // 下拉框文字用深色

                // 标题行：左边标题，右边统计信息
                ui.horizontal(|ui| {
                    // 左边：标题
                    ui.label(RichText::new("陪玩日记")
                        .font(FontId::new(32.0, egui::FontFamily::Name("cute".into())))
                        .color(text_primary));

                    // 右边：统计信息（右对齐）
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // 从右到左排列：总结余 -> 月结余 -> 日结余

                        // 总结余
                        ui.label(RichText::new(format_money(self.total_balance))
                            .font(FontId::proportional(22.0))
                            .color(green_color));
                        ui.label(RichText::new("年收入")
                            .font(FontId::proportional(13.0))
                            .color(text_secondary));

                        ui.add_space(20.0);

                        // 月结余
                        ui.label(RichText::new(format_money(self.month_balance))
                            .font(FontId::proportional(18.0))
                            .color(accent_color));

                        // 月份选择
                        let month_combo = egui::ComboBox::from_id_salt("header_month_select")
                            .width(45.0)
                            .selected_text(RichText::new(format!("{:02}", new_sel_month)).size(13.0).color(combo_text_color));
                        month_combo.show_ui(ui, |ui| {
                            for m in 1..=12u32 {
                                if ui.selectable_value(&mut new_sel_month, m, format!("{:02}月", m)).changed() {
                                    month_changed = true;
                                }
                            }
                        });

                        ui.label(RichText::new("-").size(13.0).color(text_secondary));

                        // 年份选择
                        let current_year = Local::now().year();
                        let year_combo = egui::ComboBox::from_id_salt("header_year_select")
                            .width(65.0)
                            .selected_text(RichText::new(format!("{}", new_sel_year)).size(13.0).color(combo_text_color));
                        year_combo.show_ui(ui, |ui| {
                            for y in ((current_year - 10)..=(current_year)).rev() {
                                if ui.selectable_value(&mut new_sel_year, y, format!("{}年", y)).changed() {
                                    month_changed = true;
                                }
                            }
                        });

                        ui.label(RichText::new("月收入")
                            .font(FontId::proportional(13.0))
                            .color(text_secondary));

                    });
                });

                // 处理年月选择变化
                if month_changed || new_sel_year != self.selected_year || new_sel_month != self.selected_month {
                    self.selected_year = new_sel_year;
                    self.selected_month = new_sel_month;
                    let year_month = format!("{}-{:02}", self.selected_year, self.selected_month);
                    self.month_balance = Self::calc_month_balance(&self.records, &year_month);
                    self.total_balance = Self::calc_year_balance(&self.records, self.selected_year);
                }

                ui.add_space(30.0);

                // 定义统一的卡片宽度
                let cards_width = ui.available_width();

                // ===== 输入卡片 =====
                let card_inner_w = cards_width - (layout.card_inner_margin * 2.0);
                ui.vertical(|ui| {
                    ui.set_width(cards_width);
                    egui::Frame::default()
                        .fill(card_color)
                        .corner_radius(CornerRadius::same(layout.card_rounding as u8))
                        .inner_margin(layout.card_inner_margin as i8)
                        .show(ui, |ui| {
                            ui.set_width(card_inner_w);
                        let input_height = 40.0;
                        let label_size = 13.0;
                        let input_font_size = 15.0;
                        let col_spacing = layout.col_spacing;

                        // 固定宽度元素
                        let date_width = 175.0;  // 日期选择框
                        let today_btn_width = 50.0;
                        let btn_width = 65.0;
                        let checkbox_width = 55.0;  // 结清勾选框加宽

                        // 动态分配剩余宽度给输入框
                        let fixed_total = date_width + today_btn_width + btn_width + checkbox_width;
                        let spacing_total = col_spacing * 7.0;
                        let flex_total = (card_inner_w - fixed_total - spacing_total).max(200.0);
                        // 比例分配，并设置最小宽度保护（防止从全屏游戏切换时窗口异常缩小）
                        let boss_width = (flex_total * 0.28).max(80.0);
                        let game_width = (flex_total * 0.28).max(80.0);
                        let duration_width = (flex_total * 0.18).max(50.0);
                        let income_width = (flex_total * 0.26).max(70.0);

                        let mut new_year = self.input_date.year();
                        let mut new_month = self.input_date.month();
                        let mut new_day = self.input_date.day();
                        let mut set_today = false;

                        let dark_text = Color32::from_rgb(30, 30, 35);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = col_spacing;

                            // 日期列
                            ui.vertical(|ui| {
                                ui.set_width(date_width);
                                ui.label(RichText::new("日期").color(text_secondary).size(label_size));
                                ui.add_space(4.0);
                                egui::Frame::default()
                                    .fill(input_bg)
                                    .corner_radius(CornerRadius::same(8))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(60, 65, 75)))
                                    .inner_margin(egui::Margin::symmetric(6, 0))
                                    .show(ui, |ui| {
                                        ui.set_height(input_height);
                                        ui.horizontal_centered(|ui| {
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            let current_year = Local::now().year();
                                            egui::ComboBox::from_id_salt("year_select")
                                                .width(56.0)
                                                .selected_text(RichText::new(format!("{}", new_year)).size(13.0).color(dark_text))
                                                .show_ui(ui, |ui| {
                                                    for y in (current_year - 5)..=(current_year + 1) {
                                                        ui.selectable_value(&mut new_year, y, format!("{}", y));
                                                    }
                                                });
                                            ui.label(RichText::new("-").size(13.0).color(text_secondary));
                                            egui::ComboBox::from_id_salt("month_select")
                                                .width(36.0)
                                                .selected_text(RichText::new(format!("{:02}", new_month)).size(13.0).color(dark_text))
                                                .show_ui(ui, |ui| {
                                                    for m in 1..=12u32 {
                                                        ui.selectable_value(&mut new_month, m, format!("{:02}", m));
                                                    }
                                                });
                                            ui.label(RichText::new("-").size(13.0).color(text_secondary));
                                            let max_days = days_in_month(new_year, new_month);
                                            egui::ComboBox::from_id_salt("day_select")
                                                .width(36.0)
                                                .selected_text(RichText::new(format!("{:02}", new_day)).size(13.0).color(dark_text))
                                                .show_ui(ui, |ui| {
                                                    for d in 1..=max_days {
                                                        ui.selectable_value(&mut new_day, d, format!("{:02}", d));
                                                    }
                                                });
                                        });
                                    });
                            });

                            // 今天按钮
                            ui.vertical(|ui| {
                                ui.set_width(today_btn_width);
                                ui.add_space(17.0 + 4.0);
                                let today_btn = egui::Button::new(RichText::new("今天").size(13.0).color(accent_color))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, accent_color))
                                    .corner_radius(CornerRadius::same(6));
                                if ui.add_sized([today_btn_width, input_height], today_btn).clicked() {
                                    set_today = true;
                                }
                            });

                            // 老板列（必填）
                            ui.vertical(|ui| {
                                ui.set_width(boss_width);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                    ui.label(RichText::new("老板").color(text_secondary).size(label_size));
                                    ui.label(RichText::new("*").color(danger_color).size(label_size));
                                });
                                ui.add_space(4.0);
                                // 使用 scope 限制样式修改范围
                                let boss_response = ui.scope(|ui| {
                                    if self.input_boss_error {
                                        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, danger_color);
                                        ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0, danger_color);
                                    }
                                    ui.add_sized(
                                        [boss_width, input_height],
                                        egui::TextEdit::singleline(&mut self.input_boss)
                                            .font(FontId::proportional(input_font_size))
                                            .margin(egui::Margin::symmetric(8, 8))
                                    )
                                }).inner;
                                if boss_response.gained_focus() {
                                    self.show_boss_suggestions = true;
                                    self.input_boss_error = false; // 获得焦点时清除错误状态
                                }
                                // 老板建议列表
                                let mut boss_suggestion_clicked = false;
                                if self.show_boss_suggestions && !self.boss_list.is_empty() {
                                    let input_lower = self.input_boss.to_lowercase();
                                    let suggestions: Vec<_> = self.boss_list.iter()
                                        .filter(|b| input_lower.is_empty() || b.to_lowercase().contains(&input_lower))
                                        .cloned().collect();
                                    if !suggestions.is_empty() {
                                        egui::Area::new(egui::Id::new("boss_suggestions"))
                                            .order(egui::Order::Foreground)
                                            .fixed_pos(boss_response.rect.left_bottom() + Vec2::new(0.0, 4.0))
                                            .show(ui.ctx(), |ui| {
                                                egui::Frame::default()
                                                    .fill(Color32::from_rgb(50, 55, 65))
                                                    .corner_radius(CornerRadius::same(6))
                                                    .stroke(Stroke::new(1.0, Color32::from_rgb(70, 75, 85)))
                                                    .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(60) })
                                                    .inner_margin(4)
                                                    .show(ui, |ui| {
                                                        ui.set_width(boss_width - 8.0);
                                                        // 最多显示约 6 行高度，超出部分滚动
                                                        egui::ScrollArea::vertical()
                                                            .max_height(28.0 * 6.0)
                                                            .show(ui, |ui| {
                                                                for boss in &suggestions {
                                                                    let btn = egui::Button::new(RichText::new(boss).size(14.0).color(text_primary))
                                                                        .fill(Color32::TRANSPARENT).stroke(Stroke::NONE).corner_radius(CornerRadius::same(4));
                                                                    if ui.add_sized([boss_width - 16.0, 28.0], btn).clicked() {
                                                                        self.input_boss = boss.clone();
                                                                        boss_suggestion_clicked = true;
                                                                    }
                                                                }
                                                            });
                                                    });
                                            });
                                    }
                                }
                                if boss_suggestion_clicked {
                                    self.show_boss_suggestions = false;
                                } else if self.show_boss_suggestions && !boss_response.has_focus() && ui.ctx().input(|i| i.pointer.any_click()) {
                                    self.show_boss_suggestions = false;
                                }
                            });

                            // 游戏列
                            ui.vertical(|ui| {
                                ui.set_width(game_width);
                                ui.label(RichText::new("游戏").color(text_secondary).size(label_size));
                                ui.add_space(4.0);
                                let game_response = ui.add_sized(
                                    [game_width, input_height],
                                    egui::TextEdit::singleline(&mut self.input_game)
                                        .font(FontId::proportional(input_font_size))
                                        .margin(egui::Margin::symmetric(8, 8))
                                );
                                if game_response.gained_focus() {
                                    self.show_game_suggestions = true;
                                }
                                let mut game_suggestion_clicked = false;
                                if self.show_game_suggestions && !self.game_list.is_empty() {
                                    let input_lower = self.input_game.to_lowercase();
                                    let suggestions: Vec<_> = self.game_list.iter()
                                        .filter(|g| input_lower.is_empty() || g.to_lowercase().contains(&input_lower))
                                        .take(6).cloned().collect();
                                    if !suggestions.is_empty() {
                                        egui::Area::new(egui::Id::new("game_suggestions"))
                                            .order(egui::Order::Foreground)
                                            .fixed_pos(game_response.rect.left_bottom() + Vec2::new(0.0, 4.0))
                                            .show(ui.ctx(), |ui| {
                                                egui::Frame::default()
                                                    .fill(Color32::from_rgb(50, 55, 65))
                                                    .corner_radius(CornerRadius::same(6))
                                                    .stroke(Stroke::new(1.0, Color32::from_rgb(70, 75, 85)))
                                                    .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(60) })
                                                    .inner_margin(4)
                                                    .show(ui, |ui| {
                                                        ui.set_width(game_width - 8.0);
                                                        for game in &suggestions {
                                                            let btn = egui::Button::new(RichText::new(game).size(14.0).color(text_primary))
                                                                .fill(Color32::TRANSPARENT).stroke(Stroke::NONE).corner_radius(CornerRadius::same(4));
                                                            if ui.add_sized([game_width - 16.0, 28.0], btn).clicked() {
                                                                self.input_game = game.clone();
                                                                game_suggestion_clicked = true;
                                                            }
                                                        }
                                                    });
                                            });
                                    }
                                }
                                if game_suggestion_clicked {
                                    self.show_game_suggestions = false;
                                } else if self.show_game_suggestions && !game_response.has_focus() && ui.ctx().input(|i| i.pointer.any_click()) {
                                    self.show_game_suggestions = false;
                                }
                            });

                            // 时长列
                            ui.vertical(|ui| {
                                ui.set_width(duration_width);
                                ui.label(RichText::new("时长/h").color(text_secondary).size(label_size));
                                ui.add_space(4.0);
                                ui.add_sized([duration_width, input_height],
                                    egui::TextEdit::singleline(&mut self.input_duration)
                                        .font(FontId::proportional(input_font_size))
                                        .margin(egui::Margin::symmetric(6, 8))
                                        .char_limit(5)
                                );
                            });

                            // 收入列（必填）
                            ui.vertical(|ui| {
                                ui.set_width(income_width);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                    ui.label(RichText::new("收入").color(text_secondary).size(label_size));
                                    ui.label(RichText::new("*").color(danger_color).size(label_size));
                                });
                                ui.add_space(4.0);
                                // 使用 scope 限制样式修改范围
                                let income_response = ui.scope(|ui| {
                                    if self.input_income_error {
                                        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, danger_color);
                                        ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0, danger_color);
                                    }
                                    ui.add_sized(
                                        [income_width, input_height],
                                        egui::TextEdit::singleline(&mut self.input_income)
                                            .font(FontId::proportional(input_font_size))
                                            .margin(egui::Margin::symmetric(6, 8))
                                            .char_limit(10)
                                    )
                                }).inner;
                                // 获得焦点时清除错误状态
                                if income_response.gained_focus() {
                                    self.input_income_error = false;
                                }
                            });

                            // 结清列
                            ui.vertical(|ui| {
                                ui.set_width(checkbox_width);
                                ui.label(RichText::new("结清").color(text_secondary).size(label_size));
                                ui.add_space(4.0);
                                ui.add_space(10.0);
                                ui.scope(|ui| {
                                    if self.input_settled {
                                        ui.visuals_mut().widgets.inactive.bg_fill = danger_color;
                                        ui.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::WHITE);
                                        ui.visuals_mut().widgets.hovered.bg_fill = danger_color;
                                        ui.visuals_mut().widgets.hovered.fg_stroke = Stroke::new(2.0, Color32::WHITE);
                                    }
                                    ui.add_sized([checkbox_width, 20.0], egui::Checkbox::new(&mut self.input_settled, ""));
                                });
                            });

                            // 添加按钮
                            ui.vertical(|ui| {
                                ui.set_width(btn_width);
                                ui.add_space(17.0 + 4.0);
                                let btn = egui::Button::new(RichText::new("添加").font(FontId::proportional(14.0)).color(Color32::WHITE))
                                    .fill(accent_color)
                                    .corner_radius(CornerRadius::same(6));
                                if ui.add_sized([btn_width, input_height], btn).clicked() {
                                    self.add_record();
                                }
                            });
                            }); // 结束 vertical, horizontal

                        // 处理日期变化
                        if set_today {
                            self.input_date = Local::now().date_naive();
                        } else {
                            let max_day = days_in_month(new_year, new_month);
                            let valid_day = new_day.min(max_day);
                            if let Some(date) = NaiveDate::from_ymd_opt(new_year, new_month, valid_day) {
                                self.input_date = date;
                            }
                        }
                    });
                });

                ui.add_space(24.0);

                // ===== 表格区域 =====
                ui.vertical(|ui| {
                    ui.set_width(cards_width);
                    egui::Frame::default()
                        .fill(card_color)
                        .corner_radius(CornerRadius::same(layout.card_rounding as u8))
                        .inner_margin(layout.card_inner_margin as i8)
                        .show(ui, |ui| {
                            let table_inner_w = cards_width - (layout.card_inner_margin * 2.0);
                            ui.set_width(table_inner_w);
                        let table_w = table_inner_w;
                        // 让表格占据剩余所有高度
                        let remaining_height = ui.available_height();
                        ui.set_min_height(remaining_height.max(390.0));

                        // 固定列宽
                        let col_spacing = layout.col_spacing;
                        let delete_btn_width = 60.0;
                        let settled_width = 45.0;
                        let spacing_total = col_spacing * 7.0;  // 8列有7个间距
                        let data_width = table_w - delete_btn_width - settled_width - spacing_total;
                        let col_widths = [
                            data_width * 0.15,  // 日期
                            data_width * 0.18,  // 老板
                            data_width * 0.20,  // 游戏
                            data_width * 0.10,  // 时长
                            data_width * 0.17,  // 收入
                            data_width * 0.20,  // 结余
                            settled_width,      // 结清
                            delete_btn_width,   // 操作
                        ];

                        // 表头
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = col_spacing;
                            ui.add_sized([col_widths[0], 22.0], egui::Label::new(
                                RichText::new("日期").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[1], 22.0], egui::Label::new(
                                RichText::new("老板").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[2], 22.0], egui::Label::new(
                                RichText::new("游戏").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[3], 22.0], egui::Label::new(
                                RichText::new("时长").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[4], 22.0], egui::Label::new(
                                RichText::new("收入").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[5], 22.0], egui::Label::new(
                                RichText::new("结余").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[6], 22.0], egui::Label::new(
                                RichText::new("结清").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[7], 22.0], egui::Label::new(
                                RichText::new("操作").color(text_secondary).size(14.0)
                            ));
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // 数据列表（显示选中月份的记录）
                        let selected_month_str = format!("{}-{:02}", self.selected_year, self.selected_month);
                        let filtered_records: Vec<Record> = self.records.iter()
                            .filter(|r| r.date.starts_with(&selected_month_str))
                            .cloned()
                            .collect();

                        // 计算当月累计结余（按时间正序累计，最新记录显示总累计）
                        let mut running_balances: Vec<f64> = Vec::new();
                        let total: f64 = filtered_records.iter().map(|r| r.income).sum();
                        let mut remaining = total;
                        for r in &filtered_records {
                            running_balances.push(remaining);
                            remaining -= r.income;
                        }

                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if filtered_records.is_empty() {
                                    ui.add_space(80.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(RichText::new("当月暂无记录")
                                            .color(text_secondary)
                                            .size(17.0));
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("选择其他月份或添加新记录")
                                            .color(Color32::from_rgb(100, 105, 115))
                                            .size(13.0));
                                    });
                                } else {
                                    let mut to_delete: Option<i64> = None;
                                    let mut to_toggle_settled: Option<(i64, bool)> = None;
                                    let row_height = 44.0;

                                    for (idx, record) in filtered_records.iter().enumerate() {
                                        let row_bg = if idx % 2 == 1 {
                                            Color32::from_rgb(40, 44, 52)
                                        } else {
                                            Color32::TRANSPARENT
                                        };

                                        egui::Frame::default()
                                            .fill(row_bg)
                                            .corner_radius(CornerRadius::same(6))
                                            .inner_margin(egui::Margin::symmetric(4, 6))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = col_spacing;
                                                    let text_height = row_height - 12.0;

                                                    // 日期
                                                    ui.add_sized([col_widths[0], text_height], egui::Label::new(
                                                        RichText::new(&record.date)
                                                            .color(text_primary)
                                                            .size(14.0)
                                                    ));
                                                    // 老板
                                                    ui.add_sized([col_widths[1], text_height], egui::Label::new(
                                                        RichText::new(&record.boss)
                                                            .color(text_primary)
                                                            .size(14.0)
                                                    ));
                                                    // 游戏
                                                    let game_text = record.game.as_deref().unwrap_or("-");
                                                    ui.add_sized([col_widths[2], text_height], egui::Label::new(
                                                        RichText::new(game_text)
                                                            .color(text_primary)
                                                            .size(14.0)
                                                    ));
                                                    // 时长
                                                    let duration_text = match record.duration {
                                                        Some(d) if d > 0.0 => {
                                                            if d.fract() == 0.0 {
                                                                format!("{}h", d as i32)
                                                            } else {
                                                                format!("{:.1}h", d)
                                                            }
                                                        },
                                                        _ => "-".to_string(),
                                                    };
                                                    ui.add_sized([col_widths[3], text_height], egui::Label::new(
                                                        RichText::new(duration_text)
                                                            .color(text_secondary)
                                                            .size(14.0)
                                                    ));
                                                    // 收入
                                                    ui.add_sized([col_widths[4], text_height], egui::Label::new(
                                                        RichText::new(format_income(record.income))
                                                            .color(green_color)
                                                            .size(14.0)
                                                    ));
                                                    // 结余
                                                    let running_balance = running_balances.get(idx).unwrap_or(&0.0);
                                                    ui.add_sized([col_widths[5], text_height], egui::Label::new(
                                                        RichText::new(format_money(*running_balance))
                                                            .color(text_primary)
                                                            .size(14.0)
                                                    ));

                                                    // 结清勾选框（可点击修改）
                                                    let mut settled = record.settled;
                                                    let checkbox_response = ui.scope(|ui| {
                                                        if settled {
                                                            ui.visuals_mut().widgets.inactive.bg_fill = danger_color;
                                                            ui.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::WHITE);
                                                            ui.visuals_mut().widgets.hovered.bg_fill = danger_color;
                                                            ui.visuals_mut().widgets.hovered.fg_stroke = Stroke::new(2.0, Color32::WHITE);
                                                        }
                                                        ui.add_sized([col_widths[6], text_height], egui::Checkbox::new(&mut settled, ""))
                                                    }).inner;
                                                    if checkbox_response.changed() {
                                                        to_toggle_settled = Some((record.id, settled));
                                                    }

                                                    // 删除按钮
                                                    let btn = egui::Button::new(
                                                        RichText::new("删除")
                                                            .size(12.0)
                                                            .color(danger_color)
                                                    )
                                                    .fill(Color32::TRANSPARENT)
                                                    .stroke(Stroke::new(1.0, danger_color))
                                                    .corner_radius(CornerRadius::same(5))
                                                    .min_size(Vec2::new(48.0, 26.0));

                                                    if ui.add(btn).clicked() {
                                                        to_delete = Some(record.id);
                                                    }
                                                });
                                            });
                                    }

                                    // 处理结清状态更新
                                    if let Some((id, new_settled)) = to_toggle_settled {
                                        if self.db.update_settled(id, new_settled).is_ok() {
                                            self.refresh_data();
                                        }
                                    }

                                    if let Some(id) = to_delete {
                                        self.delete_record(id);
                                    }
                                }
                            });
                    });
                }); // vertical for table card
                    }); // vertical
                }); // horizontal for centering
            });
    }
}
