# 游戏陪玩记账

一个简洁的游戏陪玩收入记账软件，使用 Rust + egui 开发。

## 功能

- 记录每日陪玩收入
- 按老板分类统计
- 日/月/总结余统计
- 老板名称自动补全
- 本地 SQLite 数据库存储

## 安装

```bash
cargo build --release
```

编译后的可执行文件位于 `target/release/jz.exe`

## 数据存储

数据库文件：`%LOCALAPPDATA%\jz\records.db`

## 依赖

- [eframe](https://github.com/emilk/egui) - GUI 框架
- [rusqlite](https://github.com/rusqlite/rusqlite) - SQLite 数据库
- [chrono](https://github.com/chronotope/chrono) - 日期时间处理

## License

MIT
