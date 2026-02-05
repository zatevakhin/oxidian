use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    Error, FileKind, FrontmatterStatus, Link, LinkKind, LinkTarget, NoteMeta, Result, Subpath,
    TaskStatus, Vault, VaultIndex, VaultPath,
};

pub struct SqliteIndexStore {
    conn: Connection,
}

impl SqliteIndexStore {
    pub fn open_default(vault: &Vault) -> Result<Self> {
        Self::open_path(Self::default_db_path(vault))
    }

    pub fn open_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let conn = Connection::open(path).map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        let mut this = Self { conn };
        this.init_schema()?;
        Ok(this)
    }

    pub fn default_db_path(vault: &Vault) -> PathBuf {
        vault
            .root()
            .join(".obsidian")
            .join("system3-obsidian.sqlite")
    }

    pub fn write_full_index(&mut self, vault: &Vault, index: &VaultIndex) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute_batch(
            "DELETE FROM links;
             DELETE FROM tasks;
             DELETE FROM tags;
             DELETE FROM notes;
             DELETE FROM files;",
        )
        .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;

        for f in index.all_files() {
            Self::upsert_path_in_tx(vault, index, &tx, &f.path)?;
        }

        tx.commit()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Ok(())
    }

    pub fn upsert_path(
        &mut self,
        vault: &Vault,
        index: &VaultIndex,
        path: &VaultPath,
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Self::upsert_path_in_tx(vault, index, &tx, path)?;
        tx.commit()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Ok(())
    }

    pub fn remove_path(&mut self, path: &VaultPath) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        let p = path.as_str_lossy();
        tx.execute("DELETE FROM links WHERE src_path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM tasks WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM tags WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM notes WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM files WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.commit()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Ok(())
    }

    pub fn counts(&self) -> Result<(usize, usize, usize, usize, usize)> {
        Ok((
            count(&self.conn, "files")?,
            count(&self.conn, "notes")?,
            count(&self.conn, "tags")?,
            count(&self.conn, "tasks")?,
            count(&self.conn, "links")?,
        ))
    }

    fn init_schema(&mut self) -> Result<()> {
        self.conn
            .execute_batch(
                "PRAGMA foreign_keys=ON;

                 CREATE TABLE IF NOT EXISTS meta(
                   key TEXT PRIMARY KEY,
                   value TEXT NOT NULL
                 );

                 CREATE TABLE IF NOT EXISTS files(
                   path TEXT PRIMARY KEY,
                   kind INTEGER NOT NULL,
                   mtime INTEGER NOT NULL,
                   size INTEGER NOT NULL
                 );

                 CREATE TABLE IF NOT EXISTS notes(
                   path TEXT PRIMARY KEY,
                   title TEXT NOT NULL,
                   aliases_json TEXT NOT NULL,
                   frontmatter_status INTEGER NOT NULL,
                   fields_json TEXT NOT NULL,
                   FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
                 );

                 CREATE TABLE IF NOT EXISTS tags(
                   tag TEXT NOT NULL,
                   path TEXT NOT NULL,
                   FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
                 );
                 CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(tag);
                 CREATE INDEX IF NOT EXISTS idx_tags_path ON tags(path);

                 CREATE TABLE IF NOT EXISTS tasks(
                   path TEXT NOT NULL,
                   line INTEGER NOT NULL,
                   status INTEGER NOT NULL,
                   text TEXT NOT NULL,
                   FOREIGN KEY(path) REFERENCES files(path) ON DELETE CASCADE
                 );
                 CREATE INDEX IF NOT EXISTS idx_tasks_path ON tasks(path);

                 CREATE TABLE IF NOT EXISTS links(
                   src_path TEXT NOT NULL,
                   line INTEGER NOT NULL,
                   col INTEGER NOT NULL,
                   kind INTEGER NOT NULL,
                   embed INTEGER NOT NULL,
                   target_type INTEGER NOT NULL,
                   target_ref TEXT NOT NULL,
                   subpath_type INTEGER,
                   subpath TEXT,
                   display TEXT,
                   raw TEXT NOT NULL,
                   FOREIGN KEY(src_path) REFERENCES files(path) ON DELETE CASCADE
                 );
                 CREATE INDEX IF NOT EXISTS idx_links_src ON links(src_path);
                ",
            )
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;

        let schema_version: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        if schema_version.is_none() {
            self.conn
                .execute(
                    "INSERT INTO meta(key,value) VALUES('schema_version','1')",
                    [],
                )
                .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        }

        Ok(())
    }

    fn upsert_path_in_tx(
        vault: &Vault,
        index: &VaultIndex,
        tx: &rusqlite::Transaction<'_>,
        path: &VaultPath,
    ) -> Result<()> {
        let Some(file) = index.file(path) else {
            return Ok(());
        };
        let p = path.as_str_lossy();
        let kind = file_kind_int(file.kind);
        let mtime = system_time_to_unix(file.mtime);
        let size = file.size as i64;

        tx.execute(
            "INSERT INTO files(path,kind,mtime,size) VALUES(?1,?2,?3,?4)
             ON CONFLICT(path) DO UPDATE SET kind=excluded.kind, mtime=excluded.mtime, size=excluded.size",
            params![p, kind, mtime, size],
        )
        .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;

        // Clear existing note-related rows.
        tx.execute("DELETE FROM notes WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM tags WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM tasks WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM links WHERE src_path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;

        if let Some(note) = index.note(path) {
            Self::insert_note_rows(tx, &p, note)?;
        } else {
            // no note rows
            let _ = vault;
        }

        Ok(())
    }

    fn insert_note_rows(tx: &rusqlite::Transaction<'_>, p: &str, note: &NoteMeta) -> Result<()> {
        let aliases_json = serde_json::to_string(&note.aliases)
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        let fields_json = serde_json::to_string(&note.fields)
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        let fm_status = frontmatter_status_int(&note.frontmatter);

        tx.execute(
            "INSERT INTO notes(path,title,aliases_json,frontmatter_status,fields_json)
             VALUES(?1,?2,?3,?4,?5)",
            params![p, note.title, aliases_json, fm_status, fields_json],
        )
        .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;

        for tag in &note.tags {
            tx.execute(
                "INSERT INTO tags(tag,path) VALUES(?1,?2)",
                params![tag.0, p],
            )
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        }

        for t in &note.tasks {
            tx.execute(
                "INSERT INTO tasks(path,line,status,text) VALUES(?1,?2,?3,?4)",
                params![p, t.line as i64, task_status_int(t.status), t.text],
            )
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        }

        for l in &note.link_occurrences {
            Self::insert_link(tx, p, l)?;
        }

        Ok(())
    }

    fn insert_link(tx: &rusqlite::Transaction<'_>, src: &str, l: &Link) -> Result<()> {
        let (target_type, target_ref) = match &l.target {
            LinkTarget::Internal { reference } => (0i64, reference.as_str()),
            LinkTarget::ExternalUrl(url) => (1i64, url.as_str()),
            LinkTarget::ObsidianUri { raw } => (2i64, raw.as_str()),
        };
        let (sub_type, sub_val) = match &l.subpath {
            None => (None, None),
            Some(Subpath::Heading(h)) => (Some(0i64), Some(h.as_str())),
            Some(Subpath::Block(b)) => (Some(1i64), Some(b.as_str())),
        };
        tx.execute(
            "INSERT INTO links(src_path,line,col,kind,embed,target_type,target_ref,subpath_type,subpath,display,raw)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                src,
                l.location.line as i64,
                l.location.column as i64,
                link_kind_int(&l.kind),
                if l.embed { 1i64 } else { 0i64 },
                target_type,
                target_ref,
                sub_type,
                sub_val,
                l.display.as_deref(),
                l.raw,
            ],
        )
        .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Ok(())
    }
}

fn count(conn: &Connection, table: &str) -> Result<usize> {
    let sql = format!("SELECT COUNT(1) FROM {table}");
    let n: i64 = conn
        .query_row(&sql, [], |r| r.get(0))
        .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
    Ok(n as usize)
}

fn file_kind_int(k: FileKind) -> i64 {
    match k {
        FileKind::Markdown => 0,
        FileKind::Canvas => 1,
        FileKind::Attachment => 2,
        FileKind::Other => 3,
    }
}

fn link_kind_int(k: &LinkKind) -> i64 {
    match k {
        LinkKind::Wiki => 0,
        LinkKind::Markdown => 1,
        LinkKind::AutoUrl => 2,
        LinkKind::ObsidianUri => 3,
    }
}

fn task_status_int(s: TaskStatus) -> i64 {
    match s {
        TaskStatus::Todo => 0,
        TaskStatus::Done => 1,
        TaskStatus::InProgress => 2,
        TaskStatus::Cancelled => 3,
        TaskStatus::Blocked => 4,
    }
}

fn frontmatter_status_int(s: &FrontmatterStatus) -> i64 {
    match s {
        FrontmatterStatus::None => 0,
        FrontmatterStatus::Valid => 1,
        FrontmatterStatus::Broken { .. } => 2,
    }
}

fn system_time_to_unix(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
