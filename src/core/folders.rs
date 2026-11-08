//! Lazily loaded folder tree of a collection (ADR-0057).
//!
//! One call returns one page of the direct subfolders of one folder, each with the
//! number of photos below it. The whole tree is never materialised.

use super::collection::Collection;
use anyhow::Result;
use rusqlite::{ToSql, types::Null};

/// Maximum number of subfolders per page and level.
pub const FOLDER_PAGE: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderNode {
    /// Name of the folder (a single path segment).
    pub name: String,
    /// Photos in this folder and all folders below it, one per photo (preferred variant).
    pub photos: usize,
    /// True if the folder contains at least one subfolder with photos.
    pub has_children: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderPage {
    pub folders: Vec<FolderNode>,
    pub more: bool,
}

/// SQL on the existing `ix_files_directory_name` index: a range over `files.dir`, joined to the
/// preferred variant in `metadata_photos` through its primary key. The same projection and the
/// same relpath prefix semantics as the folder filter of `metadata::search` apply, so the
/// counts match the number of photos shown after selecting the folder.
fn sql(root: bool) -> String {
    let bounds = if root {
        "f.dir>''"
    } else {
        "f.dir>=?1 AND f.dir<?2"
    };
    format!(
        "SELECT child,count(*),max(instr(sub,'/')>0) FROM (
            SELECT substr(f.dir,?3,instr(substr(f.dir,?3)||'/','/')-1) child, substr(f.dir,?3) sub
            FROM files f INDEXED BY ix_files_directory_name
            JOIN metadata_photos p ON p.photo_id=f.photo_id AND p.file_id=f.id
            WHERE {bounds})
         GROUP BY child ORDER BY child COLLATE NOCASE,child LIMIT ?4 OFFSET ?5"
    )
}

fn arguments(parent: &str, page: usize) -> (bool, Vec<Box<dyn ToSql>>) {
    let root = parent.is_empty();
    let prefix = if root {
        String::new()
    } else {
        format!("{parent}/")
    };
    let start = prefix.chars().count() as i64 + 1;
    let offset = page.saturating_mul(FOLDER_PAGE).min(i64::MAX as usize) as i64;
    let limit = FOLDER_PAGE as i64 + 1;
    if root {
        (
            root,
            vec![
                Box::new(Null),
                Box::new(Null),
                Box::new(start),
                Box::new(limit),
                Box::new(offset),
            ],
        )
    } else {
        // '0' is the successor of '/', so [prefix, upper) covers exactly the paths below `parent`.
        (
            root,
            vec![
                Box::new(prefix),
                Box::new(format!("{parent}0")),
                Box::new(start),
                Box::new(limit),
                Box::new(offset),
            ],
        )
    }
}

/// Direct subfolders of `parent` (`""` is the collection root, otherwise a relative path with `/`).
/// `page` counts from 0. Cancellation works through [`Collection::cancel_with`].
pub fn children(collection: &Collection, parent: &str, page: usize) -> Result<FolderPage> {
    let (root, args) = arguments(parent, page);
    let mut statement = collection.db.prepare(&sql(root))?;
    let mut rows = statement.query(rusqlite::params_from_iter(args.iter().map(|a| &**a)))?;
    let mut folders = Vec::new();
    while let Some(row) = rows.next()? {
        folders.push(FolderNode {
            name: row.get(0)?,
            photos: row.get::<_, i64>(1)? as usize,
            has_children: row.get::<_, i64>(2)? != 0,
        });
    }
    let more = folders.len() > FOLDER_PAGE;
    folders.truncate(FOLDER_PAGE);
    Ok(FolderPage { folders, more })
}

/// EXPLAIN QUERY PLAN lines of the child query, used by the regression test.
pub fn plan(collection: &Collection, parent: &str) -> Result<Vec<String>> {
    let (root, args) = arguments(parent, 0);
    let mut statement = collection
        .db
        .prepare(&format!("EXPLAIN QUERY PLAN {}", sql(root)))?;
    let rows = statement.query_map(rusqlite::params_from_iter(args.iter().map(|a| &**a)), |r| {
        r.get::<_, String>(3)
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[derive(Clone, Debug, Default)]
struct Level {
    folders: Vec<FolderNode>,
    next_page: usize,
    more: bool,
}

/// One visible line of the tree, in display order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Node {
        path: String,
        name: String,
        depth: usize,
        photos: usize,
        has_children: bool,
        expanded: bool,
    },
    /// "weitere …": the next page of the level below `parent` is still missing.
    More {
        parent: String,
        depth: usize,
    },
    Loading {
        depth: usize,
    },
    Failed {
        depth: usize,
    },
}

/// Loaded levels and expansion state of the folder tree, independent of the UI toolkit.
/// A level is only requested once its parent is expanded; pages append to a level.
#[derive(Clone, Debug, Default)]
pub struct FolderTree {
    levels: std::collections::BTreeMap<String, Level>,
    expanded: std::collections::BTreeSet<String>,
    queued: Vec<(String, usize)>,
    inflight: Vec<(String, usize)>,
    failed: std::collections::BTreeSet<String>,
}

/// Upper bound of levels per worker request.
const BATCH: usize = 24;

impl FolderTree {
    /// Forget loaded levels (contents changed) but keep what the user expanded.
    pub fn reset(&mut self) {
        let expanded = std::mem::take(&mut self.expanded);
        *self = Self::default();
        self.expanded = expanded;
    }

    /// Expands the ancestors of `folder` so that the selected folder is reachable.
    pub fn reveal(&mut self, folder: &str) {
        let mut end = 0;
        for part in folder.split('/') {
            if end > 0 {
                self.expanded.insert(folder[..end - 1].to_owned());
            }
            end += part.len() + 1;
        }
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    pub fn toggle(&mut self, path: &str) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_owned());
        }
    }

    pub fn collapse(&mut self, path: &str) {
        self.expanded.remove(path);
    }

    pub fn expand(&mut self, path: &str) {
        self.expanded.insert(path.to_owned());
    }

    /// Asks for the next page of a level ("weitere …").
    pub fn request_more(&mut self, parent: &str) {
        if let Some(level) = self.levels.get(parent)
            && level.more
        {
            let request = (parent.to_owned(), level.next_page);
            if !self.queued.contains(&request) && !self.inflight.contains(&request) {
                self.queued.push(request);
            }
        }
    }

    /// Levels to load now: the root, expanded folders and queued pages, bounded per request.
    pub fn wanted(&self) -> Vec<(String, usize)> {
        let mut wanted = Vec::new();
        for path in std::iter::once(&String::new()).chain(self.expanded.iter()) {
            if !self.levels.contains_key(path)
                && !self.failed.contains(path)
                && !self.inflight.iter().any(|(p, _)| p == path)
            {
                wanted.push((path.clone(), 0));
            }
        }
        for request in &self.queued {
            if !self.inflight.contains(request) {
                wanted.push(request.clone());
            }
        }
        wanted.truncate(BATCH);
        wanted
    }

    /// Marks a batch as handed to a worker.
    pub fn begin(&mut self, batch: &[(String, usize)]) {
        self.queued.retain(|request| !batch.contains(request));
        self.inflight = batch.to_vec();
    }

    pub fn is_loading(&self) -> bool {
        !self.inflight.is_empty()
    }

    pub fn apply(&mut self, results: Vec<(String, usize, FolderPage)>) {
        for (parent, page, loaded) in results {
            let level = self.levels.entry(parent).or_default();
            if page == 0 {
                level.folders.clear();
            } else if page != level.next_page {
                continue;
            }
            level.folders.extend(loaded.folders);
            level.next_page = page + 1;
            level.more = loaded.more;
        }
        self.inflight.clear();
    }

    /// A worker failed: do not retry the same levels in a loop.
    pub fn fail(&mut self) {
        for (parent, _) in std::mem::take(&mut self.inflight) {
            self.failed.insert(parent);
        }
    }

    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }

    pub fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }

    /// Visible rows below the root, depth first. Only loaded and expanded levels appear.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        self.collect("", 0, &mut rows);
        rows
    }

    fn collect(&self, parent: &str, depth: usize, rows: &mut Vec<Row>) {
        let Some(level) = self.levels.get(parent) else {
            rows.push(if self.failed.contains(parent) {
                Row::Failed { depth }
            } else {
                Row::Loading { depth }
            });
            return;
        };
        for node in &level.folders {
            let path = if parent.is_empty() {
                node.name.clone()
            } else {
                format!("{parent}/{}", node.name)
            };
            let expanded = node.has_children && self.expanded.contains(&path);
            rows.push(Row::Node {
                path: path.clone(),
                name: node.name.clone(),
                depth,
                photos: node.photos,
                has_children: node.has_children,
                expanded,
            });
            if expanded {
                self.collect(&path, depth + 1, rows);
            }
        }
        if level.more {
            rows.push(Row::More {
                parent: parent.to_owned(),
                depth,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(names: &[(&str, usize, bool)], more: bool) -> FolderPage {
        FolderPage {
            folders: names
                .iter()
                .map(|(name, photos, has_children)| FolderNode {
                    name: (*name).into(),
                    photos: *photos,
                    has_children: *has_children,
                })
                .collect(),
            more,
        }
    }

    #[test]
    fn levels_load_only_when_their_parent_is_expanded() {
        let mut tree = FolderTree::default();
        assert_eq!(tree.wanted(), [(String::new(), 0)]);
        tree.begin(&tree.wanted());
        assert!(tree.wanted().is_empty() && tree.is_loading());
        tree.apply(vec![(String::new(), 0, page(&[("2024", 5, true)], false))]);
        assert!(tree.wanted().is_empty());
        assert!(matches!(
            tree.rows().as_slice(),
            [Row::Node {
                expanded: false,
                ..
            }]
        ));
        tree.toggle("2024");
        assert_eq!(tree.wanted(), [("2024".to_owned(), 0)]);
        assert!(matches!(
            tree.rows().last(),
            Some(Row::Loading { depth: 1 })
        ));
        tree.begin(&tree.wanted());
        tree.apply(vec![("2024".into(), 0, page(&[("05", 4, false)], false))]);
        assert_eq!(tree.rows().len(), 2);
        tree.toggle("2024");
        assert_eq!(tree.rows().len(), 1);
    }

    #[test]
    fn reveal_expands_only_ancestors_and_pages_append() {
        let mut tree = FolderTree::default();
        tree.reveal("a/b/c");
        assert!(tree.is_expanded("a") && tree.is_expanded("a/b") && !tree.is_expanded("a/b/c"));
        let wanted = tree.wanted();
        assert_eq!(wanted.len(), 3, "{wanted:?}");
        tree.begin(&wanted);
        tree.apply(vec![(String::new(), 0, page(&[("a", 1, true)], true))]);
        assert!(
            tree.rows()
                .iter()
                .any(|row| matches!(row, Row::Loading { depth: 1 }))
        );
        tree.request_more("");
        assert!(tree.wanted().contains(&(String::new(), 1)));
        tree.begin(&[(String::new(), 1)]);
        tree.apply(vec![(String::new(), 1, page(&[("b", 2, false)], false))]);
        let names: Vec<_> = tree
            .rows()
            .into_iter()
            .filter_map(|row| match row {
                Row::Node { name, .. } => Some(name),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn failed_levels_are_not_retried_in_a_loop() {
        let mut tree = FolderTree::default();
        tree.begin(&tree.wanted());
        tree.fail();
        assert!(tree.wanted().is_empty() && tree.has_failures());
        assert!(matches!(tree.rows().as_slice(), [Row::Failed { .. }]));
        tree.retry_failed();
        assert_eq!(tree.wanted().len(), 1);
    }
}
