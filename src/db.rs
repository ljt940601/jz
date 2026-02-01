use rusqlite::{Connection, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Record {
    pub id: i64,
    pub date: String,
    pub boss: String,
    pub income: f64,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = Self::get_db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&db_path)?;
        let db = Database { conn };
        db.init()?;
        Ok(db)
    }

    fn get_db_path() -> PathBuf {
        let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("jz");
        path.push("records.db");
        path
    }

    fn init(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,
                boss TEXT NOT NULL,
                income REAL NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
            )",
            [],
        )?;
        Ok(())
    }

    pub fn add_record(&self, date: &str, boss: &str, income: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO records (date, boss, income) VALUES (?1, ?2, ?3)",
            [date, boss, &income.to_string()],
        )?;
        Ok(())
    }

    pub fn delete_record(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM records WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn get_all_records(&self) -> Result<Vec<Record>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, date, boss, income FROM records ORDER BY date DESC, id DESC"
        )?;
        let records = stmt.query_map([], |row| {
            Ok(Record {
                id: row.get(0)?,
                date: row.get(1)?,
                boss: row.get(2)?,
                income: row.get(3)?,
            })
        })?;
        records.collect()
    }

    /// 计算某个老板的结余（累计收入）
    pub fn get_boss_balance(&self, boss: &str) -> f64 {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(income), 0) FROM records WHERE boss = ?1",
                [boss],
                |row| row.get(0),
            )
            .unwrap_or(0.0)
    }

    /// 计算总结余
    pub fn get_total_balance(&self) -> f64 {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(income), 0) FROM records",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0.0)
    }

    /// 获取所有老板名称（用于自动补全）
    pub fn get_all_bosses(&self) -> Vec<String> {
        let mut stmt = self.conn
            .prepare("SELECT DISTINCT boss FROM records ORDER BY boss")
            .unwrap();
        let bosses = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        bosses
    }
}
