//! Folder tree of a collection (ADR-0057, ADR-0067).
//!
//! The complete overview (every folder with the number of photos below it) is read in one pass
//! and kept in memory; expanding and collapsing happens without database access.

use super::collection::Collection;
use anyhow::Result;

/// One folder with the photos in it and all folders below it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Node {
    name: String,
    path: String,
    photos: usize,
    children: Vec<Node>,
}

/// Photos per directory (`""` is the collection root), one row per photo (preferred variant,
/// no sidecars). One pass over the collection: the whole overview is read at once, because the
/// number of directories is small compared to the number of photos.
///
/// With the covering index `ix_files_dir_cover` (additive, see ADR 0067) the scan stays inside
/// the index; without it (read-only collection of an older version) the same result comes from
/// the relative paths in `metadata_photos`.
pub fn directories(collection: &Collection) -> Result<Vec<(String, usize)>> {
    let covered: bool = collection.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='ix_files_dir_cover')",
        [],
        |r| r.get(0),
    )?;
    let sql = if covered {
        "SELECT f.dir,count(*) FROM files f INDEXED BY ix_files_dir_cover
         JOIN metadata_photos p ON p.photo_id=f.photo_id AND p.file_id=f.id GROUP BY f.dir"
    } else {
        "SELECT rtrim(rtrim(relpath,replace(relpath,'/','')),'/'),count(*) FROM metadata_photos GROUP BY 1"
    };
    let mut statement = collection.db.prepare(sql)?;
    let mut rows = statement.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize));
    }
    Ok(out)
}

/// Last result of [`directories`] for one collection, kept on the local disk next to the
/// settings. On a slow (USB) disk the cold query takes many seconds; the cached overview is
/// shown immediately and replaced when the fresh one arrives. Never written into the collection.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cached {
    /// Normalized collection path; a cache for another collection is ignored.
    pub collection: String,
    pub photos: usize,
    pub files: usize,
    /// Seconds since the Unix epoch when the overview was read.
    pub saved: u64,
    pub directories: Vec<(String, usize)>,
}

/// Comparable form of a collection path: absolute, forward slashes, no trailing slash, lower
/// case (Windows paths are case-insensitive). Does not touch the disk.
fn normalized(root: &std::path::Path) -> String {
    let absolute = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    absolute
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

/// `<settings dir>/cache/folders-<hash>.json` for the collection at `root`.
pub fn cache_path(root: &std::path::Path) -> std::path::PathBuf {
    let hash = blake3::hash(normalized(root).as_bytes()).to_hex();
    crate::config::Settings::path()
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
        .join("cache")
        .join(format!("folders-{}.json", &hash[..16]))
}

/// Cached overview of the collection at `root`; missing, unreadable, corrupt or foreign caches
/// count as absent.
pub fn read_cache(root: &std::path::Path) -> Option<Cached> {
    let bytes = std::fs::read(cache_path(root)).ok()?;
    let cached: Cached = serde_json::from_slice(&bytes).ok()?;
    (cached.collection == normalized(root)).then_some(cached)
}

/// Stores a freshly read overview at `path` (from [`cache_path`], taken on the UI thread)
/// atomically, so a reader never sees half a file.
pub fn write_cache(
    path: &std::path::Path,
    root: &std::path::Path,
    photos: usize,
    files: usize,
    directories: &[(String, usize)],
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cache-Pfad ungültig"))?;
    std::fs::create_dir_all(parent)?;
    let cached = Cached {
        collection: normalized(root),
        photos,
        files,
        saved: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        directories: directories.to_vec(),
    };
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, &cached)?;
    file.persist(path)?;
    Ok(())
}

/// EXPLAIN QUERY PLAN lines of the directory query on the covering index (regression test).
pub fn plan(collection: &Collection) -> Result<Vec<String>> {
    let mut statement = collection.db.prepare(
        "EXPLAIN QUERY PLAN SELECT f.dir,count(*) FROM files f INDEXED BY ix_files_dir_cover
         JOIN metadata_photos p ON p.photo_id=f.photo_id AND p.file_id=f.id GROUP BY f.dir",
    )?;
    let rows = statement.query_map([], |r| r.get::<_, String>(3))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn char_class(c: char) -> u8 {
    if c.is_ascii_digit() {
        1
    } else if c.is_alphabetic() {
        2
    } else {
        0
    }
}

/// Lower-case letter without the common umlaut/accent, so `Ä` sorts next to `a`.
fn base_letter(c: char) -> char {
    match c.to_lowercase().next().unwrap_or(c) {
        'ä' | 'á' | 'à' | 'â' | 'ã' | 'å' => 'a',
        'ö' | 'ó' | 'ò' | 'ô' | 'õ' => 'o',
        'ü' | 'ú' | 'ù' | 'û' => 'u',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

fn digit_run(it: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut digits = String::new();
    while let Some(&ch) = it.peek() {
        if !ch.is_ascii_digit() {
            break;
        }
        digits.push(ch);
        it.next();
    }
    digits.trim_start_matches('0').to_owned()
}

/// Order of the Windows Explorer: symbols such as `_` first, then digits (as numbers, so `2`
/// before `10`), then letters without regard to case.
pub fn explorer_order(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => break,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let (m, n) = (digit_run(&mut x), digit_run(&mut y));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(&n));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(c), Some(d)) => {
                let order = char_class(c)
                    .cmp(&char_class(d))
                    .then_with(|| base_letter(c).cmp(&base_letter(d)));
                if order != Ordering::Equal {
                    return order;
                }
                x.next();
                y.next();
            }
        }
    }
    a.cmp(b)
}

#[derive(Default)]
struct Draft {
    photos: usize,
    children: std::collections::BTreeMap<String, Draft>,
}

fn finish(parent: &str, draft: Draft) -> Vec<Node> {
    let mut nodes: Vec<Node> = draft
        .children
        .into_iter()
        .map(|(name, child)| {
            let path = if parent.is_empty() {
                name.clone()
            } else {
                format!("{parent}/{name}")
            };
            Node {
                photos: child.photos,
                children: finish(&path, child),
                name,
                path,
            }
        })
        .collect();
    nodes.sort_by(|a, b| explorer_order(&a.name, &b.name));
    nodes
}

fn build(directories: &[(String, usize)]) -> Vec<Node> {
    let mut root = Draft::default();
    for (dir, count) in directories {
        let mut at = &mut root;
        for part in dir.split('/').filter(|part| !part.is_empty()) {
            at = at.children.entry(part.to_owned()).or_default();
            at.photos += count;
        }
    }
    finish("", root)
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
    Loading {
        depth: usize,
    },
    Failed {
        depth: usize,
    },
}

/// Folder overview and expansion state, independent of the UI toolkit. The complete tree is
/// read in one worker request and kept in memory; expanding and collapsing never touches the
/// database. After a change of the collection the old tree stays visible until the new one
/// arrives.
#[derive(Clone, Debug, Default)]
pub struct FolderTree {
    nodes: Option<Vec<Node>>,
    expanded: std::collections::BTreeSet<String>,
    stale: bool,
    loading: bool,
    failed: bool,
    /// The shown tree came from the local cache and has not been confirmed by a read yet.
    cached: bool,
}

impl FolderTree {
    /// Contents changed: read again, but keep what is shown and what the user expanded.
    pub fn reset(&mut self) {
        self.stale = true;
        self.failed = false;
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

    /// True if a worker should read the overview now.
    pub fn needs_load(&self) -> bool {
        (self.nodes.is_none() || self.stale) && !self.loading && !self.failed
    }

    /// Marks the read as handed to a worker.
    pub fn begin(&mut self) {
        self.loading = true;
        self.stale = false;
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// A read finished. Its result is shown even if the contents changed meanwhile (nothing
    /// newer exists); a change marked by [`reset`](Self::reset) during the read keeps the tree
    /// stale, so exactly one follow-up read starts.
    pub fn apply(&mut self, directories: &[(String, usize)]) {
        self.nodes = Some(build(directories));
        self.loading = false;
        self.failed = false;
        self.cached = false;
    }

    /// Shows the cached overview until the running read replaces it. Only fills an empty tree.
    pub fn show_cached(&mut self, directories: &[(String, usize)]) {
        if self.nodes.is_none() {
            self.nodes = Some(build(directories));
            self.cached = true;
        }
    }

    pub fn has_nodes(&self) -> bool {
        self.nodes.is_some()
    }

    /// A tree is shown and a read to refresh it is running.
    pub fn is_refreshing(&self) -> bool {
        self.nodes.is_some() && self.loading
    }

    /// The shown tree is the cached one, not yet confirmed by a read.
    pub fn is_cached(&self) -> bool {
        self.cached
    }

    /// A worker failed: do not retry in a loop.
    pub fn fail(&mut self) {
        self.loading = false;
        self.failed = true;
    }

    pub fn retry_failed(&mut self) {
        self.failed = false;
    }

    pub fn has_failures(&self) -> bool {
        self.failed
    }

    /// Number of folders in the tree (all levels).
    pub fn folder_count(&self) -> usize {
        fn count(nodes: &[Node]) -> usize {
            nodes.iter().map(|n| 1 + count(&n.children)).sum()
        }
        self.nodes.as_deref().map_or(0, count)
    }

    /// Visible rows below the root, depth first. Only expanded folders show their children.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        match &self.nodes {
            None => rows.push(if self.failed {
                Row::Failed { depth: 0 }
            } else {
                Row::Loading { depth: 0 }
            }),
            Some(nodes) => self.collect(nodes, 0, &mut rows),
        }
        rows
    }

    fn collect(&self, nodes: &[Node], depth: usize, rows: &mut Vec<Row>) {
        for node in nodes {
            let has_children = !node.children.is_empty();
            let expanded = has_children && self.expanded.contains(&node.path);
            rows.push(Row::Node {
                path: node.path.clone(),
                name: node.name.clone(),
                depth,
                photos: node.photos,
                has_children,
                expanded,
            });
            if expanded {
                self.collect(&node.children, depth + 1, rows);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(list: &[(&str, usize)]) -> Vec<(String, usize)> {
        list.iter().map(|(d, n)| ((*d).to_owned(), *n)).collect()
    }

    fn names(tree: &FolderTree) -> Vec<String> {
        tree.rows()
            .into_iter()
            .filter_map(|row| match row {
                Row::Node { path, .. } => Some(path),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_whole_tree_is_built_at_once_and_expansion_needs_no_loading() {
        let mut tree = FolderTree::default();
        assert!(tree.needs_load());
        tree.begin();
        assert!(!tree.needs_load() && tree.is_loading());
        assert!(matches!(
            tree.rows().as_slice(),
            [Row::Loading { depth: 0 }]
        ));
        tree.apply(&dirs(&[
            ("", 2),
            ("2024/05", 4),
            ("2024/06", 1),
            ("2025", 3),
        ]));
        assert_eq!(names(&tree), ["2024", "2025"]);
        tree.toggle("2024");
        assert_eq!(names(&tree), ["2024", "2024/05", "2024/06", "2025"]);
        assert!(!tree.needs_load());
        let Row::Node { photos, .. } = &tree.rows()[0] else {
            panic!("expected a folder row")
        };
        assert_eq!(*photos, 5, "photos below a folder are summed up");
        tree.toggle("2024");
        assert_eq!(names(&tree), ["2024", "2025"]);
        assert_eq!(tree.folder_count(), 4);
    }

    #[test]
    fn reveal_expands_only_ancestors_and_reset_keeps_the_old_tree_visible() {
        let mut tree = FolderTree::default();
        tree.reveal("a/b/c");
        assert!(tree.is_expanded("a") && tree.is_expanded("a/b") && !tree.is_expanded("a/b/c"));
        tree.begin();
        tree.apply(&dirs(&[("a/b/c", 1)]));
        assert_eq!(names(&tree), ["a", "a/b", "a/b/c"]);
        tree.reset();
        assert!(tree.needs_load());
        assert_eq!(
            names(&tree).len(),
            3,
            "old tree stays until the new one arrives"
        );
    }

    #[test]
    fn changes_during_a_read_keep_its_result_and_ask_for_exactly_one_more() {
        let mut tree = FolderTree::default();
        tree.show_cached(&dirs(&[("alt", 1)]));
        assert!(tree.is_cached() && tree.needs_load());
        tree.begin();
        assert!(tree.is_refreshing());
        tree.reset();
        tree.reset();
        assert!(!tree.needs_load(), "the running read is not replaced");
        tree.apply(&dirs(&[("neu", 1)]));
        assert_eq!(names(&tree), ["neu"]);
        assert!(!tree.is_cached() && tree.needs_load());
        tree.begin();
        tree.apply(&dirs(&[("neu", 2)]));
        assert!(!tree.needs_load());
    }

    #[test]
    fn the_cache_roundtrips_and_ignores_corrupt_or_foreign_files() -> Result<()> {
        let root = tempfile::tempdir()?;
        assert_eq!(read_cache(root.path()), None);
        write_cache(
            &cache_path(root.path()),
            root.path(),
            5,
            7,
            &dirs(&[("a", 2), ("a/b", 3)]),
        )?;
        let cached = read_cache(root.path()).expect("cache written");
        assert_eq!((cached.photos, cached.files), (5, 7));
        assert_eq!(cached.directories, dirs(&[("a", 2), ("a/b", 3)]));
        assert!(
            cache_path(root.path()).starts_with(
                crate::config::Settings::path()
                    .parent()
                    .expect("settings dir")
            ),
            "the cache lives next to the settings, never in the collection"
        );
        let other = tempfile::tempdir()?;
        std::fs::copy(cache_path(root.path()), cache_path(other.path()))?;
        assert_eq!(
            read_cache(other.path()),
            None,
            "cache of another collection"
        );
        std::fs::write(cache_path(root.path()), b"{ kaputt")?;
        assert_eq!(read_cache(root.path()), None);
        Ok(())
    }

    #[test]
    fn failed_reads_are_not_retried_in_a_loop() {
        let mut tree = FolderTree::default();
        tree.begin();
        tree.fail();
        assert!(!tree.needs_load() && tree.has_failures());
        assert!(matches!(tree.rows().as_slice(), [Row::Failed { .. }]));
        tree.retry_failed();
        assert!(tree.needs_load());
    }

    #[test]
    fn folders_are_sorted_like_the_explorer() {
        let mut list = vec![
            "Zebra",
            "apfel",
            "10 Urlaub",
            "2 Urlaub",
            "_Sortiert",
            "-neu",
            "2024",
            "02",
            "Äpfel",
            "_alt",
            "2024-05",
        ];
        list.sort_by(|a, b| explorer_order(a, b));
        assert_eq!(
            list,
            [
                "-neu",
                "_alt",
                "_Sortiert",
                "02",
                "2 Urlaub",
                "10 Urlaub",
                "2024",
                "2024-05",
                "apfel",
                "Äpfel",
                "Zebra"
            ]
        );
    }
}
