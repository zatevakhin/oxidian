use std::path::{Path, PathBuf};
#[cfg(feature = "similarity")]
use std::sync::Once;

#[cfg(feature = "similarity")]
use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{Connection, OptionalExtension, params};
#[cfg(feature = "similarity")]
use sqlite_vec::sqlite3_vec_init;
#[cfg(feature = "similarity")]
use tracing::{debug, info, trace, warn};
#[cfg(feature = "similarity")]
use zerocopy::AsBytes;

#[cfg(feature = "similarity")]
use crate::embeddings::{EmbeddingModel, clean_markdown_for_embedding, hash_text};
use crate::{
    Error, FileKind, FrontmatterStatus, Link, LinkKind, LinkTarget, NoteMeta, Result, Subpath,
    TaskStatus, Vault, VaultIndex, VaultPath,
};

pub struct SqliteIndexStore {
    conn: Connection,
}

#[cfg(feature = "similarity")]
static VEC_INIT: Once = Once::new();

#[cfg(feature = "similarity")]
fn init_vec_extension() {
    VEC_INIT.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
        debug!("sqlite-vec extension registered");
    });
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
        #[cfg(feature = "similarity")]
        init_vec_extension();
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
        #[cfg(feature = "similarity")]
        {
            tx.execute_batch(
                "DELETE FROM note_embeddings;
                 DELETE FROM note_embedding_meta;",
            )
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        }

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
        #[cfg(feature = "similarity")]
        {
            tx.execute("DELETE FROM note_embeddings WHERE path=?1", params![p])
                .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
            tx.execute("DELETE FROM note_embedding_meta WHERE path=?1", params![p])
                .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Ok(())
    }

    #[cfg(feature = "similarity")]
    pub(crate) fn ensure_embeddings(
        &mut self,
        vault: &Vault,
        index: &VaultIndex,
        model: &EmbeddingModel,
    ) -> Result<()> {
        let existing = self.existing_embedding_paths()?;
        let existing_set: std::collections::HashSet<VaultPath> = existing.into_iter().collect();
        let current_set: std::collections::HashSet<VaultPath> =
            index.notes_iter_paths().cloned().collect();
        info!(
            total_notes = current_set.len(),
            existing_embeddings = existing_set.len(),
            "embedding refresh start"
        );

        let mut removed = 0usize;
        let mut up_to_date = 0usize;
        let mut updated = 0usize;

        for path in existing_set.difference(&current_set) {
            trace!(path = path.as_str_lossy(), "removing embedding");
            self.remove_embedding(path)?;
            removed += 1;
        }

        for path in current_set.iter() {
            trace!(path = path.as_str_lossy(), "processing embedding");
            let abs = vault.to_abs(path);
            let text = match std::fs::read_to_string(&abs) {
                Ok(v) => v,
                Err(err) => {
                    warn!(path = abs.display().to_string(), error = %err, "failed to read note for embedding");
                    return Err(Error::io(&abs, err));
                }
            };
            let cleaned = clean_markdown_for_embedding(&text);
            let hash = hash_text(&cleaned);
            let stored_hash = self.embedding_hash(path)?;
            if stored_hash.as_deref() == Some(hash.as_str()) {
                trace!(path = path.as_str_lossy(), "embedding up-to-date");
                up_to_date += 1;
                continue;
            }
            let embedding = match model.embed_text(&cleaned) {
                Ok(v) => v,
                Err(err) => {
                    warn!(path = path.as_str_lossy(), error = %err, "embedding failed");
                    return Err(err);
                }
            };
            self.upsert_embedding(path, &embedding, &hash)?;
            trace!(path = path.as_str_lossy(), "embedding stored");
            updated += 1;
        }

        info!(removed, up_to_date, updated, "embedding refresh complete");

        Ok(())
    }

    #[cfg(feature = "similarity")]
    pub fn embedding_for_path(&self, path: &VaultPath) -> Result<Option<Vec<f32>>> {
        let p = path.as_str_lossy();
        let bytes: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT embedding FROM note_embeddings WHERE path=?1",
                params![p],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        Ok(Some(bytes_to_f32(&bytes)))
    }

    #[cfg(feature = "similarity")]
    pub fn knn_for_embedding(
        &self,
        embedding_bytes: &[u8],
        limit: usize,
    ) -> Result<Vec<(VaultPath, f32)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT path, distance FROM note_embeddings WHERE embedding MATCH ?1 ORDER BY distance LIMIT ?2",
            )
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        let rows = stmt
            .query_map(params![embedding_bytes, limit as i64], |r| {
                let path: String = r.get(0)?;
                let distance: f32 = r.get(1)?;
                Ok((path, distance))
            })
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;

        let mut out = Vec::new();
        for row in rows {
            let (path, distance) = row.map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
            let vp = VaultPath::try_from(Path::new(&path))?;
            out.push((vp, distance));
        }
        Ok(out)
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
        debug!("sqlite base schema ready");

        #[cfg(feature = "similarity")]
        {
            self.conn
                .execute_batch(
                    "CREATE VIRTUAL TABLE IF NOT EXISTS note_embeddings USING vec0(
                       embedding float[384],
                       path TEXT
                     );

                     CREATE TABLE IF NOT EXISTS note_embedding_meta(
                       path TEXT PRIMARY KEY,
                       content_hash TEXT NOT NULL,
                       updated_at INTEGER NOT NULL
                     );
                     CREATE INDEX IF NOT EXISTS idx_note_embedding_meta_hash ON note_embedding_meta(content_hash);",
                )
                .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
            debug!("sqlite similarity schema ready");
        }

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

    #[cfg(feature = "similarity")]
    fn embedding_hash(&self, path: &VaultPath) -> Result<Option<String>> {
        let p = path.as_str_lossy();
        let hash: Option<String> = self
            .conn
            .query_row(
                "SELECT content_hash FROM note_embedding_meta WHERE path=?1",
                params![p],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Ok(hash)
    }

    #[cfg(feature = "similarity")]
    fn upsert_embedding(&mut self, path: &VaultPath, embedding: &[f32], hash: &str) -> Result<()> {
        let p = path.as_str_lossy();
        let now = system_time_to_unix(std::time::SystemTime::now());
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;

        tx.execute("DELETE FROM note_embeddings WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute(
            "INSERT INTO note_embeddings(path, embedding) VALUES(?1, ?2)",
            params![p, embedding.as_bytes()],
        )
        .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute(
            "INSERT INTO note_embedding_meta(path, content_hash, updated_at)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET content_hash=excluded.content_hash, updated_at=excluded.updated_at",
            params![p, hash, now],
        )
        .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;

        tx.commit()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Ok(())
    }

    #[cfg(feature = "similarity")]
    fn remove_embedding(&mut self, path: &VaultPath) -> Result<()> {
        let p = path.as_str_lossy();
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM note_embeddings WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.execute("DELETE FROM note_embedding_meta WHERE path=?1", params![p])
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        tx.commit()
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        Ok(())
    }

    #[cfg(feature = "similarity")]
    fn existing_embedding_paths(&self) -> Result<Vec<VaultPath>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM note_embedding_meta")
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| {
                let path: String = r.get(0)?;
                Ok(path)
            })
            .map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            let path = row.map_err(|e| Error::InvalidVaultPath(e.to_string()))?;
            if let Ok(vp) = VaultPath::try_from(Path::new(&path)) {
                out.push(vp);
            }
        }
        Ok(out)
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

#[cfg(feature = "similarity")]
fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let mut arr = [0u8; 4];
        arr.copy_from_slice(chunk);
        out.push(f32::from_le_bytes(arr));
    }
    out
}
