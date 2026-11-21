//! Ordnerbaum einer Sammlung (ADR-0057, ADR-0067).
//!
//! Die ganze Übersicht (jeder Ordner mit der Zahl der Fotos darunter) wird in einem Durchgang
//! gelesen und im Speicher gehalten; Auf- und Zuklappen braucht keinen Datenbankzugriff.

use super::collection::Collection;
use anyhow::Result;

/// Ein Ordner mit seinen Fotos und allen Ordnern darunter.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Node {
    name: String,
    path: String,
    photos: usize,
    children: Vec<Node>,
}

/// Fotos je Verzeichnis (`""` ist der Sammlungsordner), eine Zeile je Foto (bevorzugte
/// Variante, keine Begleitdateien). Die ganze Übersicht wird auf einmal gelesen, weil es im
/// Vergleich zu den Fotos wenige Verzeichnisse gibt.
///
/// Normalerweise liest das die zusätzliche Summentabelle `folder_counts` (eine Zeile je Ordner,
/// von Triggern auf `metadata_photos` aktuell gehalten; siehe `migrations::FOLDER_COUNTS`):
/// wenige Seiten statt tausender verstreuter auf einer langsamen Platte. Eine Sammlung, die
/// diese Version nie schreibend geöffnet hat, hat die Tabelle nicht; dann zählt
/// [`scan_directories`] direkt.
pub fn directories(collection: &Collection) -> Result<Vec<(String, usize)>> {
    let summarized: bool = collection.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='folder_counts')",
        [],
        |r| r.get(0),
    )?;
    if summarized {
        query(collection, "SELECT dir,photos FROM folder_counts")
    } else {
        scan_directories(collection)
    }
}

/// Zählt die Fotos je Verzeichnis aus der Projektion selbst (Rückfall und Referenz für
/// `folder_counts`). Mit dem abdeckenden Index `ix_files_dir_cover` (zusätzlich, siehe ADR 0067)
/// bleibt das Lesen im Index; ohne ihn kommt dasselbe Ergebnis aus den relativen Pfaden in
/// `metadata_photos`.
pub fn scan_directories(collection: &Collection) -> Result<Vec<(String, usize)>> {
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
    query(collection, sql)
}

fn query(collection: &Collection, sql: &str) -> Result<Vec<(String, usize)>> {
    let mut statement = collection.db.prepare(sql)?;
    let mut rows = statement.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize));
    }
    Ok(out)
}

/// Zeilen von EXPLAIN QUERY PLAN der Verzeichnisabfrage auf dem abdeckenden Index
/// (Regressionstest).
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

/// Kleinbuchstabe ohne die üblichen Umlaute/Akzente, damit `Ä` neben `a` einsortiert wird.
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

/// Reihenfolge des Windows-Explorers: Zeichen wie `_` zuerst, dann Ziffern (als Zahlen, also
/// `2` vor `10`), dann Buchstaben ohne Rücksicht auf Groß- und Kleinschreibung.
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

/// Eine sichtbare Zeile des Baums, in Anzeigereihenfolge.
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

/// Ordnerübersicht und Aufklappzustand, unabhängig vom UI-Toolkit. Der ganze Baum wird in
/// einem Worker-Auftrag gelesen und im Speicher gehalten; Auf- und Zuklappen berührt nie die
/// Datenbank. Nach einer Änderung der Sammlung bleibt der alte Baum sichtbar, bis der neue
/// da ist.
#[derive(Clone, Debug, Default)]
pub struct FolderTree {
    nodes: Option<Vec<Node>>,
    expanded: std::collections::BTreeSet<String>,
    stale: bool,
    loading: bool,
    failed: bool,
}

impl FolderTree {
    /// Inhalt geändert: neu lesen, aber Angezeigtes und Aufgeklapptes behalten.
    pub fn reset(&mut self) {
        self.stale = true;
        self.failed = false;
    }

    /// Klappt die Vorfahren von `folder` auf, damit der gewählte Ordner erreichbar ist.
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

    /// Soll ein Worker die Übersicht jetzt lesen?
    pub fn needs_load(&self) -> bool {
        (self.nodes.is_none() || self.stale) && !self.loading && !self.failed
    }

    /// Merkt, dass das Lesen an einen Worker übergeben ist.
    pub fn begin(&mut self) {
        self.loading = true;
        self.stale = false;
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Ein Lesen ist fertig. Sein Ergebnis wird gezeigt, auch wenn sich der Inhalt inzwischen
    /// geändert hat (Neueres gibt es nicht); eine während des Lesens mit [`reset`](Self::reset)
    /// gemerkte Änderung lässt den Baum veraltet, sodass genau ein weiteres Lesen startet.
    pub fn apply(&mut self, directories: &[(String, usize)]) {
        self.nodes = Some(build(directories));
        self.loading = false;
        self.failed = false;
    }

    /// Ein Baum wird gezeigt, und ein Lesen zum Auffrischen läuft.
    pub fn is_refreshing(&self) -> bool {
        self.nodes.is_some() && self.loading
    }

    /// Ein Worker ist gescheitert: nicht in einer Schleife wiederholen.
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

    /// Zahl der Ordner im Baum (alle Ebenen).
    pub fn folder_count(&self) -> usize {
        fn count(nodes: &[Node]) -> usize {
            nodes.iter().map(|n| 1 + count(&n.children)).sum()
        }
        self.nodes.as_deref().map_or(0, count)
    }

    /// Sichtbare Zeilen unter der Wurzel, Tiefe zuerst. Nur aufgeklappte Ordner zeigen ihre
    /// Unterordner.
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
        tree.begin();
        tree.apply(&dirs(&[("alt", 1)]));
        tree.reset();
        assert!(tree.needs_load());
        tree.begin();
        assert!(
            tree.is_refreshing(),
            "the old tree stays visible while it is read again"
        );
        tree.reset();
        tree.reset();
        assert!(!tree.needs_load(), "the running read is not replaced");
        tree.apply(&dirs(&[("neu", 1)]));
        assert_eq!(names(&tree), ["neu"]);
        assert!(tree.needs_load());
        tree.begin();
        tree.apply(&dirs(&[("neu", 2)]));
        assert!(!tree.needs_load());
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
