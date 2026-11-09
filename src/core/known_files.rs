//! In-memory view of the inventory for one index run (ADR 0064).
//!
//! The scan used to ask SQLite per new file for move candidates and variant partners; none of
//! those columns is indexed, so every question was a full table scan. The answers only depend on
//! the inventory as it was before the run plus the run's own changes, so they are loaded once
//! and kept current here. Results equal the former per-file queries.
use super::{collection::family_directory, files, jobs::Control};
use anyhow::Result;
use rusqlite::Connection;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// An inventory row whose path was not visited by the walk: the only possible move source.
#[derive(Clone, Debug)]
pub struct Unseen {
    slot: usize,
    pub id: i64,
    pub size: i64,
    pub mtime: i64,
    pub relpath: String,
    pub inode: Option<String>,
    pub full_hash: Option<String>,
    pub stem: String,
    pub dir: String,
    photo: Option<i64>,
}

pub struct KnownFiles {
    unseen: Vec<Option<Unseen>>,
    by_inode: HashMap<(String, i64), Vec<usize>>,
    hashed_by_size: HashMap<i64, Vec<usize>>,
    /// folded stem -> (file id, photo id, folded family directory), ordered by file id
    families: HashMap<String, Vec<(i64, i64, String)>>,
}
impl KnownFiles {
    pub fn load(
        db: &Connection,
        walked: &[String],
        extension_folders: &[String],
        control: &Control,
    ) -> Result<Self> {
        let walked: HashSet<&str> = walked.iter().map(String::as_str).collect();
        let rows: usize = db.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?;
        control.progress(0, rows);
        let mut known = Self {
            unseen: Vec::new(),
            by_inode: HashMap::new(),
            hashed_by_size: HashMap::new(),
            families: HashMap::new(),
        };
        let mut statement = db.prepare(
            "SELECT id,photo_id,size,mtime_ns,relpath,inode,full_hash,stem,dir FROM files ORDER BY id",
        )?;
        let mut cursor = statement.query([])?;
        let mut count = 0;
        while let Some(r) = cursor.next()? {
            count += 1;
            if count % 2048 == 0 {
                control.checkpoint()?;
                control.progress(count, rows);
            }
            let row = Unseen {
                slot: 0,
                id: r.get(0)?,
                photo: r.get(1)?,
                size: r.get(2)?,
                mtime: r.get(3)?,
                relpath: r.get(4)?,
                inode: r.get(5)?,
                full_hash: r.get(6)?,
                stem: r.get(7)?,
                dir: r.get(8)?,
            };
            if let Some(photo) = row.photo {
                known
                    .families
                    .entry(files::fold(&row.stem))
                    .or_default()
                    .push((
                        row.id,
                        photo,
                        files::fold(&family_directory(&row.dir, extension_folders)),
                    ));
            }
            if !walked.contains(row.relpath.as_str()) {
                known.push_unseen(row);
            }
        }
        Ok(known)
    }
    fn push_unseen(&mut self, mut row: Unseen) {
        row.slot = self.unseen.len();
        if let Some(inode) = &row.inode {
            self.by_inode
                .entry((inode.clone(), row.size))
                .or_default()
                .push(row.slot);
        }
        if row.full_hash.is_some() {
            self.hashed_by_size
                .entry(row.size)
                .or_default()
                .push(row.slot);
        }
        self.unseen.push(Some(row));
    }
    fn gone(&self, root: &Path, slots: &[usize]) -> Vec<Unseen> {
        slots
            .iter()
            .filter_map(|slot| self.unseen[*slot].as_ref())
            .filter(|row| !root.join(&row.relpath).exists())
            .cloned()
            .collect()
    }
    /// The single vanished file with this inode and size, if unambiguous.
    pub fn moved_by_inode(&self, root: &Path, inode: Option<&str>, size: i64) -> Option<Unseen> {
        let slots = self.by_inode.get(&(inode?.to_string(), size))?;
        let mut found = self.gone(root, slots);
        (found.len() == 1).then(|| found.remove(0))
    }
    pub fn has_hashed(&self, size: i64) -> bool {
        self.hashed_by_size.contains_key(&size)
    }
    /// Vanished files of this size that have a stored full hash.
    pub fn hashed_unseen(&self, root: &Path, size: i64) -> Vec<Unseen> {
        self.hashed_by_size
            .get(&size)
            .map(|slots| self.gone(root, slots))
            .unwrap_or_default()
    }
    /// A matched row now lives at a new path: it is no longer a candidate and its family
    /// entry follows its new stem and directory.
    pub fn relocate(
        &mut self,
        found: &Unseen,
        stem: &str,
        dir: &str,
        extension_folders: &[String],
    ) {
        self.unseen[found.slot] = None;
        let Some(photo) = found.photo else { return };
        if let Some(list) = self.families.get_mut(&files::fold(&found.stem)) {
            list.retain(|entry| entry.0 != found.id);
        }
        let list = self.families.entry(files::fold(stem)).or_default();
        let at = list.partition_point(|entry| entry.0 < found.id);
        list.insert(
            at,
            (
                found.id,
                photo,
                files::fold(&family_directory(dir, extension_folders)),
            ),
        );
    }
    /// The photo of the oldest file with this stem in the same family directory.
    pub fn family_photo(&self, stem: &str, family: &str) -> Option<i64> {
        let family = files::fold(family);
        self.families
            .get(&files::fold(stem))?
            .iter()
            .find(|entry| entry.2 == family)
            .map(|entry| entry.1)
    }
    pub fn add(&mut self, id: i64, photo: i64, stem: &str, family: &str) {
        self.families
            .entry(files::fold(stem))
            .or_default()
            .push((id, photo, files::fold(family)));
    }
}
