#![windows_subsystem = "windows"]

mod db;

use chrono::{Local, NaiveDate, Datelike};
use db::{Database, Record};
use eframe::egui::{self, Color32, FontId, RichText, Vec2, Rounding, Stroke};
use std::fs::File;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn get_lock_file_path() -> PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("jz");
    std::fs::create_dir_all(&path).ok();
    path.push(".lock");
    path
}

fn try_lock() -> Option<File> {
    use std::io::Write;

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
        use std::mem::zeroed;

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
            .with_inner_size([980.0, 800.0])
            .with_min_inner_size([960.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "记账本",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Box::new(App::new())
        }),
    )
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\msyh.ttc") {
        fonts.font_data.insert(
            "msyh".to_owned(),
            egui::FontData::from_owned(font_data).into(),
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

    ctx.set_fonts(fonts);
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

    message: String,
    message_is_error: bool,
    message_timer: f32,

    // 计时器
    timer_running: bool,
    timer_start_instant: Option<Instant>,
    timer_accumulated: Duration,
    timer_ended: bool,  // 是否已结束（结束后才能重置）
}

impl App {
    fn new() -> Self {
        let db = Database::new().expect("无法初始化数据库");
        let records = db.get_all_records().unwrap_or_default();
        let total_balance = db.get_total_balance();
        let today = Local::now().date_naive();
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
            message: String::new(),
            message_is_error: false,
            message_timer: 0.0,
            timer_running: false,
            timer_start_instant: None,
            timer_accumulated: Duration::ZERO,
            timer_ended: false,
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

    fn calc_month_balance(records: &[Record], year_month: &str) -> f64 {
        records.iter()
            .filter(|r| r.date.starts_with(year_month))
            .map(|r| r.income)
            .sum()
    }

    fn refresh_data(&mut self) {
        self.records = self.db.get_all_records().unwrap_or_default();
        self.total_balance = self.db.get_total_balance();
        let today = Local::now().date_naive();
        self.day_balance = Self::calc_day_balance(&self.records, &today.format("%Y-%m-%d").to_string());
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

        if self.input_boss.trim().is_empty() {
            self.show_message("请输入老板名称", true);
            return;
        }

        let income: f64 = match self.input_income.trim().parse::<f64>() {
            Ok(v) if v > 0.0 && v.is_finite() => v,
            _ => {
                self.show_message("请输入有效金额", true);
                return;
            }
        };

        // 检查单笔金额上限
        if income > MAX_INCOME {
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

        // 计时器运行时持续刷新
        if self.timer_running {
            ctx.request_repaint();
        }

        // 颜色定义
        let bg_color = Color32::from_rgb(25, 28, 32);
        let card_color = Color32::from_rgb(35, 39, 45);
        let input_bg = Color32::from_rgb(45, 50, 58);
        let accent_color = Color32::from_rgb(64, 169, 255);
        let green_color = Color32::from_rgb(82, 196, 126);
        let text_primary = Color32::from_rgb(230, 230, 235);
        let text_secondary = Color32::from_rgb(140, 145, 155);
        let danger_color = Color32::from_rgb(220, 80, 80);

        // ===== 底部计时器栏（固定在底部）=====
        egui::TopBottomPanel::bottom("timer_panel")
            .frame(egui::Frame::default().fill(bg_color).inner_margin(egui::Margin { left: 32.0, right: 32.0, top: 8.0, bottom: 16.0 }))
            .show(ctx, |ui| {
                // 与内容区域等宽居中
                let content_width = 880.0;
                let available = ui.available_width();
                let side_margin = ((available - content_width) / 2.0 - 35.0).max(0.0);

                ui.horizontal(|ui| {
                    ui.add_space(side_margin);
                    ui.vertical(|ui| {
                        ui.set_width(content_width);
                        egui::Frame::default()
                            .fill(card_color)
                            .rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(20.0, 14.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                            // 计算当前显示时间
                            let elapsed = if self.timer_running {
                                if let Some(start) = self.timer_start_instant {
                                    self.timer_accumulated + start.elapsed()
                                } else {
                                    self.timer_accumulated
                                }
                            } else {
                                self.timer_accumulated
                            };

                            let total_secs = elapsed.as_secs();
                            let hours = total_secs / 3600;
                            let minutes = (total_secs % 3600) / 60;
                            let seconds = total_secs % 60;
                            let time_str = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);

                            // 判断计时器状态
                            let is_initial = !self.timer_running && self.timer_accumulated.is_zero() && !self.timer_ended;
                            let is_running = self.timer_running;
                            let is_paused = !self.timer_running && !self.timer_accumulated.is_zero() && !self.timer_ended;
                            let is_ended = self.timer_ended;

                            // 左侧：标签
                            ui.label(RichText::new("计时").size(14.0).color(text_secondary));
                            ui.add_space(12.0);

                            // 时间显示
                            let time_color = if is_running {
                                accent_color
                            } else if is_paused {
                                Color32::from_rgb(230, 180, 80)
                            } else if is_ended {
                                text_primary  // 已结束显示白色
                            } else {
                                text_secondary
                            };
                            ui.label(RichText::new(time_str)
                                .font(FontId::monospace(24.0))
                                .color(time_color));

                            ui.add_space(20.0);

                            // 按钮区域
                            let btn_height = 30.0;
                            let btn_width = 56.0;

                            // 开始按钮（仅初始状态可用）
                            if is_initial {
                                let start_btn = egui::Button::new(RichText::new("开始").size(13.0).color(Color32::WHITE))
                                    .fill(green_color)
                                    .rounding(Rounding::same(6.0));
                                if ui.add_sized([btn_width, btn_height], start_btn).clicked() {
                                    self.timer_running = true;
                                    self.timer_start_instant = Some(Instant::now());
                                    self.timer_ended = false;
                                }
                            } else {
                                let disabled_btn = egui::Button::new(RichText::new("开始").size(13.0).color(Color32::from_rgb(80, 85, 95)))
                                    .fill(Color32::from_rgb(45, 48, 55))
                                    .rounding(Rounding::same(6.0));
                                ui.add_sized([btn_width, btn_height], disabled_btn);
                            }

                            ui.add_space(6.0);

                            // 暂停/继续按钮（运行中或暂停中可用）
                            if is_running {
                                let pause_btn = egui::Button::new(RichText::new("暂停").size(13.0).color(Color32::WHITE))
                                    .fill(Color32::from_rgb(230, 180, 80))
                                    .rounding(Rounding::same(6.0));
                                if ui.add_sized([btn_width, btn_height], pause_btn).clicked() {
                                    if let Some(start) = self.timer_start_instant {
                                        self.timer_accumulated += start.elapsed();
                                    }
                                    self.timer_running = false;
                                    self.timer_start_instant = None;
                                }
                            } else if is_paused {
                                let resume_btn = egui::Button::new(RichText::new("继续").size(13.0).color(Color32::WHITE))
                                    .fill(accent_color)
                                    .rounding(Rounding::same(6.0));
                                if ui.add_sized([btn_width, btn_height], resume_btn).clicked() {
                                    self.timer_running = true;
                                    self.timer_start_instant = Some(Instant::now());
                                }
                            } else {
                                let disabled_btn = egui::Button::new(RichText::new("暂停").size(13.0).color(Color32::from_rgb(80, 85, 95)))
                                    .fill(Color32::from_rgb(45, 48, 55))
                                    .rounding(Rounding::same(6.0));
                                ui.add_sized([btn_width, btn_height], disabled_btn);
                            }

                            ui.add_space(6.0);

                            // 结束按钮（运行中或暂停中可用，结束后禁用）
                            if is_running || is_paused {
                                let end_btn = egui::Button::new(RichText::new("结束").size(13.0).color(danger_color))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, danger_color))
                                    .rounding(Rounding::same(6.0));
                                if ui.add_sized([btn_width, btn_height], end_btn).clicked() {
                                    // 结束：停止计时但保留时间
                                    if let Some(start) = self.timer_start_instant {
                                        self.timer_accumulated += start.elapsed();
                                    }
                                    self.timer_running = false;
                                    self.timer_start_instant = None;
                                    self.timer_ended = true;
                                }
                            } else {
                                let disabled_btn = egui::Button::new(RichText::new("结束").size(13.0).color(Color32::from_rgb(80, 85, 95)))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(60, 65, 75)))
                                    .rounding(Rounding::same(6.0));
                                ui.add_sized([btn_width, btn_height], disabled_btn);
                            }

                            ui.add_space(6.0);

                            // 重置按钮（仅结束后可用）
                            if is_ended {
                                let reset_btn = egui::Button::new(RichText::new("重置").size(13.0).color(text_secondary))
                                    .fill(input_bg)
                                    .rounding(Rounding::same(6.0));
                                if ui.add_sized([btn_width, btn_height], reset_btn).clicked() {
                                    self.timer_accumulated = Duration::ZERO;
                                    self.timer_ended = false;
                                }
                            } else {
                                let disabled_btn = egui::Button::new(RichText::new("重置").size(13.0).color(Color32::from_rgb(80, 85, 95)))
                                    .fill(Color32::from_rgb(45, 48, 55))
                                    .rounding(Rounding::same(6.0));
                                ui.add_sized([btn_width, btn_height], disabled_btn);
                            }
                                });
                            });
                    });
                });
            });

        // 设置全局样式
        let mut style = (*ctx.style()).clone();
        style.visuals.widgets.inactive.bg_fill = input_bg;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(60, 65, 75));
        style.visuals.widgets.inactive.rounding = Rounding::same(8.0);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(55, 60, 70);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(50, 55, 65);
        style.visuals.selection.bg_fill = accent_color;
        ctx.set_style(style);

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(bg_color).inner_margin(32.0))
            .show(ctx, |ui| {
                // 固定内容宽度，居中显示
                let content_width = 880.0;
                let available = ui.available_width();
                // 减少左边距来补偿egui布局的偏移
                let side_margin = ((available - content_width) / 2.0 - 35.0).max(0.0);

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
                        .font(FontId::proportional(28.0))
                        .color(text_primary));

                    // 右边：统计信息（右对齐）
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // 从右到左排列：总结余 -> 月结余 -> 日结余

                        // 总结余
                        ui.label(RichText::new(format_money(self.total_balance))
                            .font(FontId::proportional(22.0))
                            .color(green_color));
                        ui.label(RichText::new("总结余")
                            .font(FontId::proportional(13.0))
                            .color(text_secondary));

                        ui.add_space(20.0);

                        // 月结余
                        ui.label(RichText::new(format_money(self.month_balance))
                            .font(FontId::proportional(18.0))
                            .color(accent_color));

                        // 月份选择
                        let month_combo = egui::ComboBox::from_id_source("header_month_select")
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
                        let year_combo = egui::ComboBox::from_id_source("header_year_select")
                            .width(65.0)
                            .selected_text(RichText::new(format!("{}", new_sel_year)).size(13.0).color(combo_text_color));
                        year_combo.show_ui(ui, |ui| {
                            for y in ((current_year - 10)..=(current_year)).rev() {
                                if ui.selectable_value(&mut new_sel_year, y, format!("{}年", y)).changed() {
                                    month_changed = true;
                                }
                            }
                        });

                        ui.label(RichText::new("月结余")
                            .font(FontId::proportional(13.0))
                            .color(text_secondary));

                        ui.add_space(20.0);

                        // 日结余
                        ui.label(RichText::new(format_money(self.day_balance))
                            .font(FontId::proportional(18.0))
                            .color(text_primary));
                        ui.label(RichText::new("日结余")
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
                }

                ui.add_space(30.0);

                // 定义统一的卡片宽度
                let cards_width = ui.available_width();

                // ===== 输入卡片 =====
                let card_inner_w = cards_width - 44.0;
                ui.vertical(|ui| {
                    ui.set_width(cards_width);
                    egui::Frame::default()
                        .fill(card_color)
                        .rounding(Rounding::same(14.0))
                        .inner_margin(22.0)
                        .show(ui, |ui| {
                            ui.set_width(card_inner_w);
                        let input_height = 40.0;
                        let label_size = 13.0;
                        let input_font_size = 15.0;
                        let col_spacing = 10.0;

                        // 固定宽度元素
                        let date_width = 175.0;  // 日期选择框
                        let today_btn_width = 50.0;
                        let btn_width = 65.0;
                        let checkbox_width = 55.0;  // 结清勾选框加宽

                        // 动态分配剩余宽度给输入框
                        let fixed_total = date_width + today_btn_width + btn_width + checkbox_width;
                        let spacing_total = col_spacing * 7.0;
                        let flex_total = (card_inner_w - fixed_total - spacing_total).max(200.0);
                        // 比例总和为1.0，确保不超出宽度
                        let boss_width = flex_total * 0.28;
                        let game_width = flex_total * 0.28;
                        let duration_width = flex_total * 0.18;
                        let income_width = flex_total * 0.26;

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
                                    .rounding(Rounding::same(8.0))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(60, 65, 75)))
                                    .inner_margin(egui::Margin::symmetric(6.0, 0.0))
                                    .show(ui, |ui| {
                                        ui.set_height(input_height);
                                        ui.horizontal_centered(|ui| {
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            let current_year = Local::now().year();
                                            egui::ComboBox::from_id_source("year_select")
                                                .width(56.0)
                                                .selected_text(RichText::new(format!("{}", new_year)).size(13.0).color(dark_text))
                                                .show_ui(ui, |ui| {
                                                    for y in (current_year - 5)..=(current_year + 1) {
                                                        ui.selectable_value(&mut new_year, y, format!("{}", y));
                                                    }
                                                });
                                            ui.label(RichText::new("-").size(13.0).color(text_secondary));
                                            egui::ComboBox::from_id_source("month_select")
                                                .width(36.0)
                                                .selected_text(RichText::new(format!("{:02}", new_month)).size(13.0).color(dark_text))
                                                .show_ui(ui, |ui| {
                                                    for m in 1..=12u32 {
                                                        ui.selectable_value(&mut new_month, m, format!("{:02}", m));
                                                    }
                                                });
                                            ui.label(RichText::new("-").size(13.0).color(text_secondary));
                                            let max_days = days_in_month(new_year, new_month);
                                            egui::ComboBox::from_id_source("day_select")
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
                                    .rounding(Rounding::same(6.0));
                                if ui.add_sized([today_btn_width, input_height], today_btn).clicked() {
                                    set_today = true;
                                }
                            });

                            // 老板列
                            ui.vertical(|ui| {
                                ui.set_width(boss_width);
                                ui.label(RichText::new("老板").color(text_secondary).size(label_size));
                                ui.add_space(4.0);
                                let boss_response = ui.add_sized(
                                    [boss_width, input_height],
                                    egui::TextEdit::singleline(&mut self.input_boss)
                                        .font(FontId::proportional(input_font_size))
                                        .margin(Vec2::new(8.0, 8.0))
                                );
                                if boss_response.gained_focus() {
                                    self.show_boss_suggestions = true;
                                }
                                // 老板建议列表
                                let mut boss_suggestion_clicked = false;
                                if self.show_boss_suggestions && !self.boss_list.is_empty() {
                                    let input_lower = self.input_boss.to_lowercase();
                                    let suggestions: Vec<_> = self.boss_list.iter()
                                        .filter(|b| input_lower.is_empty() || b.to_lowercase().contains(&input_lower))
                                        .take(6).cloned().collect();
                                    if !suggestions.is_empty() {
                                        egui::Area::new(egui::Id::new("boss_suggestions"))
                                            .order(egui::Order::Foreground)
                                            .fixed_pos(boss_response.rect.left_bottom() + Vec2::new(0.0, 4.0))
                                            .show(ui.ctx(), |ui| {
                                                egui::Frame::default()
                                                    .fill(Color32::from_rgb(50, 55, 65))
                                                    .rounding(Rounding::same(6.0))
                                                    .stroke(Stroke::new(1.0, Color32::from_rgb(70, 75, 85)))
                                                    .shadow(egui::epaint::Shadow { offset: Vec2::new(0.0, 2.0), blur: 8.0, spread: 0.0, color: Color32::from_black_alpha(60) })
                                                    .inner_margin(4.0)
                                                    .show(ui, |ui| {
                                                        ui.set_width(boss_width - 8.0);
                                                        for boss in &suggestions {
                                                            let btn = egui::Button::new(RichText::new(boss).size(14.0).color(text_primary))
                                                                .fill(Color32::TRANSPARENT).stroke(Stroke::NONE).rounding(Rounding::same(4.0));
                                                            if ui.add_sized([boss_width - 16.0, 28.0], btn).clicked() {
                                                                self.input_boss = boss.clone();
                                                                boss_suggestion_clicked = true;
                                                            }
                                                        }
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
                                        .margin(Vec2::new(8.0, 8.0))
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
                                                    .rounding(Rounding::same(6.0))
                                                    .stroke(Stroke::new(1.0, Color32::from_rgb(70, 75, 85)))
                                                    .shadow(egui::epaint::Shadow { offset: Vec2::new(0.0, 2.0), blur: 8.0, spread: 0.0, color: Color32::from_black_alpha(60) })
                                                    .inner_margin(4.0)
                                                    .show(ui, |ui| {
                                                        ui.set_width(game_width - 8.0);
                                                        for game in &suggestions {
                                                            let btn = egui::Button::new(RichText::new(game).size(14.0).color(text_primary))
                                                                .fill(Color32::TRANSPARENT).stroke(Stroke::NONE).rounding(Rounding::same(4.0));
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
                                        .margin(Vec2::new(6.0, 8.0))
                                        .char_limit(5)
                                );
                            });

                            // 收入列
                            ui.vertical(|ui| {
                                ui.set_width(income_width);
                                ui.label(RichText::new("收入").color(text_secondary).size(label_size));
                                ui.add_space(4.0);
                                ui.add_sized([income_width, input_height],
                                    egui::TextEdit::singleline(&mut self.input_income)
                                        .font(FontId::proportional(input_font_size))
                                        .margin(Vec2::new(6.0, 8.0))
                                        .char_limit(10)
                                );
                            });

                            // 结清列
                            ui.vertical(|ui| {
                                ui.set_width(checkbox_width);
                                ui.label(RichText::new("结清").color(text_secondary).size(label_size));
                                ui.add_space(4.0);
                                ui.add_space(10.0);
                                ui.add_sized([checkbox_width, 20.0], egui::Checkbox::new(&mut self.input_settled, ""));
                            });

                            // 添加按钮
                            ui.vertical(|ui| {
                                ui.set_width(btn_width);
                                ui.add_space(17.0 + 4.0);
                                let btn = egui::Button::new(RichText::new("添加").font(FontId::proportional(14.0)).color(Color32::WHITE))
                                    .fill(accent_color)
                                    .rounding(Rounding::same(6.0));
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

                //消息提示（浮动显示）
                // if !self.message.is_empty() {
                //     ui.add_space(8.0);
                //     let color = if self.message_is_error { danger_color } else { green_color };
                //     ui.label(RichText::new(&self.message).color(color).size(14.0));
                //     ui.add_space(16.0);
                // } else {
                    ui.add_space(24.0);
                // }

                // ===== 表格区域 =====
                egui::Frame::default()
                    .fill(card_color)
                    .rounding(Rounding::same(14.0))
                    .inner_margin(22.0)
                    .show(ui, |ui| {
                        ui.set_width(cards_width - 44.0);  // 强制固定宽度，与输入卡片一致
                        let table_w = cards_width - 44.0;
                        // 让表格占据剩余所有高度
                        let remaining_height = ui.available_height();
                        ui.set_min_height(remaining_height.max(400.0));

                        // 固定列宽
                        let delete_btn_width = 60.0;
                        let settled_width = 45.0;
                        let table_padding = 30.0;
                        let data_width = table_w - delete_btn_width - settled_width - table_padding;
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
                                            .rounding(Rounding::same(6.0))
                                            .inner_margin(egui::Margin::symmetric(4.0, 6.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
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
                                                    let checkbox_response = ui.add_sized([col_widths[6], text_height], egui::Checkbox::new(&mut settled, ""));
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
                                                    .rounding(Rounding::same(5.0))
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
                    }); // vertical
                }); // horizontal for centering
            });
    }
}
