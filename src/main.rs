#![windows_subsystem = "windows"]

mod db;

use chrono::{Local, NaiveDate, Datelike};
use db::{Database, Record};
use eframe::egui::{self, Color32, FontId, RichText, Vec2, Rounding, Stroke};
use std::fs::File;
use std::path::PathBuf;

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
            .with_inner_size([900.0, 700.0])
            .with_min_inner_size([800.0, 550.0]),
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

    // 月结余选择器
    selected_year: i32,
    selected_month: u32,

    input_date: NaiveDate,
    input_boss: String,
    input_income: String,
    show_boss_suggestions: bool,

    message: String,
    message_is_error: bool,
    message_timer: f32,
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

        Self {
            db,
            records,
            total_balance,
            day_balance,
            month_balance,
            boss_balances,
            boss_list,
            selected_year: today.year(),
            selected_month: today.month(),
            input_date: today,
            input_boss: String::new(),
            input_income: String::new(),
            show_boss_suggestions: false,
            message: String::new(),
            message_is_error: false,
            message_timer: 0.0,
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

        let date_str = self.input_date.format("%Y-%m-%d").to_string();
        match self.db.add_record(&date_str, self.input_boss.trim(), income) {
            Ok(_) => {
                self.show_message(&format!("已添加 ¥{:.2}", income), false);
                self.input_boss.clear();
                self.input_income.clear();
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

        // 颜色定义
        let bg_color = Color32::from_rgb(25, 28, 32);
        let card_color = Color32::from_rgb(35, 39, 45);
        let input_bg = Color32::from_rgb(45, 50, 58);
        let accent_color = Color32::from_rgb(64, 169, 255);
        let green_color = Color32::from_rgb(82, 196, 126);
        let text_primary = Color32::from_rgb(230, 230, 235);
        let text_secondary = Color32::from_rgb(140, 145, 155);
        let danger_color = Color32::from_rgb(220, 80, 80);

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
            .frame(egui::Frame::default().fill(bg_color).inner_margin(40.0))
            .show(ctx, |ui| {
                let _w = ui.available_width();

                // ===== 顶部标题区 =====
                let mut month_changed = false;
                let mut new_sel_year = self.selected_year;
                let mut new_sel_month = self.selected_month;
                let combo_text_color = Color32::from_rgb(30, 30, 35); // 下拉框文字用深色

                // 标题行：左边标题，右边统计信息
                ui.horizontal(|ui| {
                    // 左边：标题
                    ui.label(RichText::new("游戏陪玩记账")
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

                // ===== 输入卡片 =====
                egui::Frame::default()
                    .fill(card_color)
                    .rounding(Rounding::same(14.0))
                    .inner_margin(24.0)
                    .show(ui, |ui| {
                        let inner_w = ui.available_width();

                        let input_height = 42.0;
                        let label_size = 14.0;
                        let input_font_size = 16.0;

                        let mut new_year = self.input_date.year();
                        let mut new_month = self.input_date.month();
                        let mut new_day = self.input_date.day();
                        let mut set_today = false;

                        ui.horizontal(|ui| {
                            let col_spacing = 20.0;
                            let btn_width = 90.0;
                            let today_btn_width = 60.0;
                            let date_area_width = 240.0;
                            let remaining = inner_w - date_area_width - today_btn_width - btn_width - col_spacing * 5.0;
                            let field_width = remaining / 2.0;

                            // 日期选择
                            ui.vertical(|ui| {
                                ui.label(RichText::new("日期").color(text_secondary).size(label_size));
                                ui.add_space(6.0);

                                // 日期选择框容器
                                let dark_text = Color32::from_rgb(30, 30, 35);
                                egui::Frame::default()
                                    .fill(input_bg)
                                    .rounding(Rounding::same(8.0))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(60, 65, 75)))
                                    .inner_margin(egui::Margin::symmetric(8.0, 0.0))
                                    .show(ui, |ui| {
                                        ui.set_height(input_height);
                                        ui.horizontal_centered(|ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;

                                            // 年份选择
                                            let current_year = Local::now().year();
                                            egui::ComboBox::from_id_source("year_select")
                                                .width(72.0)
                                                .selected_text(RichText::new(format!("{}", new_year)).size(input_font_size).color(dark_text))
                                                .show_ui(ui, |ui| {
                                                    for y in (current_year - 5)..=(current_year + 1) {
                                                        ui.selectable_value(&mut new_year, y, format!("{}", y));
                                                    }
                                                });

                                            ui.label(RichText::new("-").size(input_font_size).color(text_secondary));

                                            // 月份选择
                                            egui::ComboBox::from_id_source("month_select")
                                                .width(52.0)
                                                .selected_text(RichText::new(format!("{:02}", new_month)).size(input_font_size).color(dark_text))
                                                .show_ui(ui, |ui| {
                                                    for m in 1..=12u32 {
                                                        ui.selectable_value(&mut new_month, m, format!("{:02}", m));
                                                    }
                                                });

                                            ui.label(RichText::new("-").size(input_font_size).color(text_secondary));

                                            // 日期选择
                                            let max_days = days_in_month(new_year, new_month);
                                            egui::ComboBox::from_id_source("day_select")
                                                .width(52.0)
                                                .selected_text(RichText::new(format!("{:02}", new_day)).size(input_font_size).color(dark_text))
                                                .show_ui(ui, |ui| {
                                                    for d in 1..=max_days {
                                                        ui.selectable_value(&mut new_day, d, format!("{:02}", d));
                                                    }
                                                });
                                        });
                                    });
                            });

                            ui.add_space(col_spacing - 12.0);

                            // 今天按钮
                            ui.vertical(|ui| {
                                ui.add_space(20.0 + 6.0);
                                let today_btn = egui::Button::new(
                                    RichText::new("今天").size(14.0).color(accent_color)
                                )
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::new(1.0, accent_color))
                                .rounding(Rounding::same(8.0));

                                if ui.add_sized([today_btn_width, input_height], today_btn).clicked() {
                                    set_today = true;
                                }
                            });

                            ui.add_space(col_spacing);

                            // 老板（带自动补全）
                            ui.vertical(|ui| {
                                ui.label(RichText::new("老板").color(text_secondary).size(label_size));
                                ui.add_space(6.0);

                                let boss_response = ui.add_sized(
                                    [field_width, input_height],
                                    egui::TextEdit::singleline(&mut self.input_boss)
                                        .font(FontId::proportional(input_font_size))
                                        .margin(Vec2::new(12.0, 8.0))
                                );

                                // 输入框获得焦点时显示建议
                                if boss_response.gained_focus() {
                                    self.show_boss_suggestions = true;
                                }

                                // 浮动显示建议列表
                                let mut suggestion_clicked = false;
                                if self.show_boss_suggestions && !self.boss_list.is_empty() {
                                    let input_lower = self.input_boss.to_lowercase();
                                    let suggestions: Vec<_> = self.boss_list.iter()
                                        .filter(|b| input_lower.is_empty() || b.to_lowercase().contains(&input_lower))
                                        .take(6)
                                        .cloned()
                                        .collect();

                                    if !suggestions.is_empty() {
                                        let popup_pos = boss_response.rect.left_bottom() + Vec2::new(0.0, 4.0);

                                        egui::Area::new(egui::Id::new("boss_suggestions"))
                                            .order(egui::Order::Foreground)
                                            .fixed_pos(popup_pos)
                                            .show(ui.ctx(), |ui| {
                                                egui::Frame::default()
                                                    .fill(Color32::from_rgb(50, 55, 65))
                                                    .rounding(Rounding::same(6.0))
                                                    .stroke(Stroke::new(1.0, Color32::from_rgb(70, 75, 85)))
                                                    .shadow(egui::epaint::Shadow {
                                                        offset: Vec2::new(0.0, 2.0),
                                                        blur: 8.0,
                                                        spread: 0.0,
                                                        color: Color32::from_black_alpha(60),
                                                    })
                                                    .inner_margin(4.0)
                                                    .show(ui, |ui| {
                                                        ui.set_width(field_width - 8.0);
                                                        for boss in &suggestions {
                                                            let btn = egui::Button::new(
                                                                RichText::new(boss).size(14.0).color(text_primary)
                                                            )
                                                            .fill(Color32::TRANSPARENT)
                                                            .stroke(Stroke::NONE)
                                                            .rounding(Rounding::same(4.0));

                                                            if ui.add_sized([field_width - 16.0, 28.0], btn).clicked() {
                                                                self.input_boss = boss.clone();
                                                                suggestion_clicked = true;
                                                            }
                                                        }
                                                    });
                                            });
                                    }
                                }

                                // 点击建议后隐藏，或者点击其他地方时隐藏
                                if suggestion_clicked {
                                    self.show_boss_suggestions = false;
                                } else if self.show_boss_suggestions && !boss_response.has_focus() {
                                    // 检查鼠标是否点击了其他地方
                                    let clicked_elsewhere = ui.ctx().input(|i| i.pointer.any_click());
                                    if clicked_elsewhere {
                                        self.show_boss_suggestions = false;
                                    }
                                }
                            });

                            ui.add_space(col_spacing);

                            // 收入（限制输入长度，最多10字符：100000.00）
                            ui.vertical(|ui| {
                                ui.label(RichText::new("收入").color(text_secondary).size(label_size));
                                ui.add_space(6.0);
                                ui.add_sized(
                                    [field_width, input_height],
                                    egui::TextEdit::singleline(&mut self.input_income)
                                        .font(FontId::proportional(input_font_size))
                                        .margin(Vec2::new(12.0, 8.0))
                                        .char_limit(10)
                                );
                            });

                            ui.add_space(col_spacing);

                            // 按钮
                            ui.vertical(|ui| {
                                ui.add_space(20.0 + 6.0);
                                let btn = egui::Button::new(
                                    RichText::new("添加")
                                        .font(FontId::proportional(16.0))
                                        .color(Color32::WHITE)
                                )
                                .fill(accent_color)
                                .rounding(Rounding::same(8.0));

                                if ui.add_sized([btn_width, input_height], btn).clicked() {
                                    self.add_record();
                                }
                            });
                        });

                        // 处理日期变化
                        if set_today {
                            self.input_date = Local::now().date_naive();
                        } else {
                            // 确保日期有效
                            let max_day = days_in_month(new_year, new_month);
                            let valid_day = new_day.min(max_day);
                            if let Some(date) = NaiveDate::from_ymd_opt(new_year, new_month, valid_day) {
                                self.input_date = date;
                            }
                        }

                        });

                // 消息提示（浮动显示）
                if !self.message.is_empty() {
                    ui.add_space(8.0);
                    let color = if self.message_is_error { danger_color } else { green_color };
                    ui.label(RichText::new(&self.message).color(color).size(14.0));
                    ui.add_space(16.0);
                } else {
                    ui.add_space(24.0);
                }

                // ===== 表格区域 =====
                egui::Frame::default()
                    .fill(card_color)
                    .rounding(Rounding::same(14.0))
                    .inner_margin(20.0)
                    .show(ui, |ui| {
                        let table_w = ui.available_width();
                        ui.set_min_height(350.0);

                        // 固定列宽
                        let delete_btn_width = 70.0;
                        let table_padding = 40.0;
                        let data_width = table_w - delete_btn_width - table_padding;
                        let col_widths = [
                            data_width * 0.22,  // 日期
                            data_width * 0.28,  // 老板
                            data_width * 0.24,  // 收入
                            data_width * 0.26,  // 结余
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
                                RichText::new("收入").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[3], 22.0], egui::Label::new(
                                RichText::new("结余").color(text_secondary).size(14.0)
                            ));
                            ui.add_sized([col_widths[4], 22.0], egui::Label::new(
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

                                                    ui.add_sized([col_widths[0], text_height], egui::Label::new(
                                                        RichText::new(&record.date)
                                                            .color(text_primary)
                                                            .size(15.0)
                                                    ));
                                                    ui.add_sized([col_widths[1], text_height], egui::Label::new(
                                                        RichText::new(&record.boss)
                                                            .color(text_primary)
                                                            .size(15.0)
                                                    ));
                                                    ui.add_sized([col_widths[2], text_height], egui::Label::new(
                                                        RichText::new(format_income(record.income))
                                                            .color(green_color)
                                                            .size(15.0)
                                                    ));
                                                    let running_balance = running_balances.get(idx).unwrap_or(&0.0);
                                                    ui.add_sized([col_widths[3], text_height], egui::Label::new(
                                                        RichText::new(format_money(*running_balance))
                                                            .color(text_primary)
                                                            .size(15.0)
                                                    ));

                                                    // 删除按钮
                                                    let btn = egui::Button::new(
                                                        RichText::new("删除")
                                                            .size(13.0)
                                                            .color(danger_color)
                                                    )
                                                    .fill(Color32::TRANSPARENT)
                                                    .stroke(Stroke::new(1.0, danger_color))
                                                    .rounding(Rounding::same(5.0))
                                                    .min_size(Vec2::new(52.0, 28.0));

                                                    if ui.add(btn).clicked() {
                                                        to_delete = Some(record.id);
                                                    }
                                                });
                                            });
                                    }

                                    if let Some(id) = to_delete {
                                        self.delete_record(id);
                                    }
                                }
                            });
                    });
            });
    }
}
