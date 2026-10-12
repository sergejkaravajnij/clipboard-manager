use rusqlite::{Connection, params};
use std::path::Path;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const PAGE_SIZE: usize = 100;
pub const PREVIEW_CHARS: usize = 8000;

pub struct Store(pub Connection);

#[derive(Debug)]
pub struct Row {
    pub id: i64,
    pub label: String,
}

pub struct Format {
    pub id: u32,
    pub name: String,
    pub kind: i32,
    pub data: Vec<u8>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let indexed: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='entries_fts')",
            [],
            |r| r.get(0),
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(3))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             PRAGMA cache_size=-2048;
             PRAGMA mmap_size=0;
             CREATE TABLE IF NOT EXISTS entries (
               id INTEGER PRIMARY KEY, created INTEGER NOT NULL,
               title TEXT NOT NULL DEFAULT '', search TEXT NOT NULL DEFAULT '',
               preview TEXT NOT NULL DEFAULT '', bytes INTEGER NOT NULL DEFAULT 0,
               formats INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS formats (
               entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
               ordinal INTEGER NOT NULL, format_id INTEGER NOT NULL,
               name TEXT NOT NULL, kind INTEGER NOT NULL, data BLOB NOT NULL,
               PRIMARY KEY(entry_id, ordinal));
             CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
               search, content='entries', content_rowid='id', tokenize='trigram case_sensitive 1');
             CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
               INSERT INTO entries_fts(rowid,search) VALUES(new.id,new.search);
             END;
             CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
               INSERT INTO entries_fts(entries_fts,rowid,search) VALUES('delete',old.id,old.search);
             END;
             CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE OF search ON entries BEGIN
               INSERT INTO entries_fts(entries_fts,rowid,search) VALUES('delete',old.id,old.search);
               INSERT INTO entries_fts(rowid,search) VALUES(new.id,new.search);
             END;
             PRAGMA foreign_keys=ON;",
        )?;
        if !indexed {
            conn.execute_batch("INSERT INTO entries_fts(entries_fts) VALUES('rebuild');")?;
        }
        Ok(Self(conn))
    }

    pub fn list(&self, filter: &str, offset: usize) -> Result<Vec<Row>> {
        let lower = filter.to_lowercase();
        let indexed = lower.chars().count() >= 3 && !lower.contains('\0');
        let pattern = if indexed {
            format!("\"{}\"", lower.replace('"', "\"\""))
        } else {
            format!(
                "%{}%",
                lower
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        };
        let predicate = if indexed {
            "id IN (SELECT rowid FROM entries_fts WHERE entries_fts MATCH ?)"
        } else {
            "search LIKE ? ESCAPE '\\'"
        };
        let sql = format!(
            "SELECT id, strftime('%d.%m %H:%M', created, 'unixepoch', 'localtime'), title, bytes FROM entries WHERE {predicate} ORDER BY id DESC LIMIT ? OFFSET ?"
        );
        let mut stmt = self.0.prepare(&sql)?;
        let rows = stmt.query_map(params![pattern, PAGE_SIZE as i64, offset as i64], |r| {
            let bytes: i64 = r.get(3)?;
            Ok(Row {
                id: r.get(0)?,
                label: format!(
                    "{}   {}   [{}]",
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    size_label(bytes)
                ),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn preview(&self, id: i64) -> Result<String> {
        Ok(self
            .0
            .query_row("SELECT preview FROM entries WHERE id=?", [id], |r| r.get(0))?)
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .0
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?)
    }

    pub fn format_keys(&self, id: i64) -> Result<Vec<i64>> {
        let mut s = self
            .0
            .prepare("SELECT ordinal FROM formats WHERE entry_id=? ORDER BY ordinal")?;
        Ok(s.query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn format(&self, id: i64, ordinal: i64) -> Result<Format> {
        Ok(self.0.query_row(
            "SELECT format_id, name, kind, data FROM formats WHERE entry_id=? AND ordinal=?",
            params![id, ordinal],
            |r| {
                Ok(Format {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    data: r.get(3)?,
                })
            },
        )?)
    }

    pub fn image(&self, entry: i64) -> Result<Option<Vec<u8>>> {
        use rusqlite::OptionalExtension;
        Ok(self.0.query_row("SELECT data FROM formats WHERE entry_id=? AND format_id IN (8,17) ORDER BY format_id DESC LIMIT 1", [entry], |r| r.get(0)).optional()?)
    }
}

fn size_label(bytes: i64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} МБ", bytes as f64 / 1048576.0)
    } else if bytes >= 1024 {
        format!("{} КБ", bytes / 1024)
    } else {
        format!("{bytes} Б")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_history_unicode_literal_search_and_paging() -> Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!("clipboard-store-test-{stamp}.sqlite"));
        {
            let store = Store::open(&path)?;
            for n in 0..205 {
                let text = if n == 4 {
                    "ПрИвЕт 100%_\\".to_string()
                } else {
                    format!("item {n}")
                };
                store.0.execute(
                    "INSERT INTO entries(created,title,search,preview) VALUES(0,?,?,?)",
                    params![text, text.to_lowercase(), text],
                )?;
            }
            assert_eq!(store.list("", 0)?.len(), 100);
            assert_eq!(store.list("", 200)?.len(), 5);
            assert_eq!(store.list("ПРИВЕТ", 0)?.len(), 1);
            assert_eq!(store.list("%_\\", 0)?.len(), 1);
            assert!(store.list("absent", 0)?.is_empty());
        }
        {
            let store = Store::open(&path)?;
            assert_eq!(store.count()?, 205);
        }
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn indexed_search_large_history_without_loading_payloads() -> Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("clipboard-search-test-{stamp}"));
        std::fs::create_dir_all(&directory)?;
        let store = Store::open(&directory.join("history.sqlite"))?;
        store.0.execute_batch("BEGIN;")?;
        for n in 0..10000 {
            let text = format!("Строка {n}: needle-{n} конец текста");
            store.0.execute(
                "INSERT INTO entries(created,title,search,preview) VALUES(0,?,?,?)",
                params![text, text.to_lowercase(), text],
            )?;
        }
        // A 64-MiB history object stays in formats, absent from all list queries.
        store.0.execute(
            "INSERT INTO formats VALUES(1,0,49152,'Large binary',0,zeroblob(67108864))",
            [],
        )?;
        store
            .0
            .execute_batch("COMMIT; PRAGMA wal_checkpoint(TRUNCATE); PRAGMA shrink_memory;")?;
        let start = std::time::Instant::now();
        let found = store.list("needle-9999", 0)?;
        let elapsed = start.elapsed();
        assert_eq!(found.len(), 1);
        assert!(found[0].label.contains("9999"));
        assert_eq!(store.list("КОНЕЦ ТЕКСТА", 0)?.len(), 100);
        assert_eq!(store.list("99", 0)?.len(), 100);
        assert_eq!(store.list("", 0)?.len(), 100);
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "Indexed filter took {elapsed:?}"
        );
        println!("Indexed search in 10,000 records with 64 MiB payload: {elapsed:?}");
        drop(store);
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    #[ignore = "Creates an on-disk fixture for external release-process verification"]
    fn create_release_runtime_fixture() -> Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts");
        let directory = root.join(format!("runtime-fixture-{stamp}"));
        std::fs::create_dir_all(&directory)?;
        let store = Store::open(&directory.join("history.sqlite"))?;
        store.0.execute_batch("BEGIN;")?;
        for n in 0..10000 {
            let text = format!("Тест истории {n}: needle-{n}");
            store.0.execute(
                "INSERT INTO entries(created,title,search,preview) VALUES(0,?,?,?)",
                params![text, text.to_lowercase(), text],
            )?;
        }
        store.0.execute(
            "INSERT INTO formats VALUES(1,0,49152,'Large binary',0,zeroblob(67108864))",
            [],
        )?;
        store
            .0
            .execute_batch("COMMIT; PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(store);
        std::fs::write(
            root.join("runtime-fixture-path.txt"),
            directory.to_string_lossy().as_bytes(),
        )?;
        println!("Created release fixture: {}", directory.display());
        Ok(())
    }
}
