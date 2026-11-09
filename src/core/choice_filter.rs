//! Tippfilter für Auswahllisten (ADR 0063): reine, getestete Logik ohne Oberfläche.
//!
//! Der Suchtext wird wie Namen in der Sammlung gefaltet (`files::fold`, also ohne
//! Groß-/Kleinschreibung) und zusätzlich in zwei Schreibweisen von Umlauten und
//! Akzenten verglichen: „Müller“ findet man mit `mü`, `mu` und `mue`.
use super::files;

/// Zwei vergleichbare Schreibweisen eines Textes: Akzente entfernt (`ä`→`a`) und
/// deutsche Umschrift (`ä`→`ae`). Beide sind bereits gefaltet.
pub fn forms(text: &str) -> [String; 2] {
    let folded = files::fold(text);
    let mut plain = String::with_capacity(folded.len());
    let mut german = String::with_capacity(folded.len());
    for c in folded.chars() {
        match c {
            'ä' => {
                plain.push('a');
                german.push_str("ae");
            }
            'ö' => {
                plain.push('o');
                german.push_str("oe");
            }
            'ü' => {
                plain.push('u');
                german.push_str("ue");
            }
            other => {
                let base = strip_accent(other);
                plain.push(base);
                german.push(base);
            }
        }
    }
    [plain, german]
}

/// Grundbuchstabe häufiger Akzentzeichen; alles andere bleibt unverändert.
fn strip_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'å' | 'ā' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'ō' | 'ő' => 'o',
        'ś' | 'š' | 'ş' => 's',
        'ù' | 'ú' | 'û' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        'ł' => 'l',
        'ř' => 'r',
        'ď' => 'd',
        'ť' => 't',
        other => other,
    }
}

/// Sortierschlüssel: gefaltet, Akzente entfernt, damit „Ärztin“ bei „A“ steht.
pub fn sort_key(text: &str) -> String {
    let [plain, _] = forms(text);
    plain
}

/// Güte eines Treffers; kleiner ist besser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// Der Text beginnt mit dem Suchwort.
    Prefix,
    /// Ein späteres Wort des Textes beginnt mit dem Suchwort.
    WordPrefix,
    /// Das Suchwort steht irgendwo im Text.
    Substring,
}

/// Vorbereiteter Suchtext: Wörter durch Leerzeichen getrennt, alle müssen passen.
#[derive(Clone, Debug, Default)]
pub struct Matcher {
    tokens: Vec<[String; 2]>,
}

impl Matcher {
    pub fn new(query: &str) -> Self {
        Self {
            tokens: query.split_whitespace().map(forms).collect(),
        }
    }
    /// Leere Suche trifft alles.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
    /// Güte des ersten Suchworts oder `None`, wenn irgendein Suchwort fehlt.
    pub fn rank(&self, text: &str) -> Option<Rank> {
        if self.tokens.is_empty() {
            return Some(Rank::Prefix);
        }
        let candidate = forms(text);
        let mut first = None;
        for token in &self.tokens {
            let best = (0..2)
                .filter_map(|i| token_rank(&token[i], &candidate[i]))
                .min()?;
            first.get_or_insert(best);
        }
        first
    }
}

fn token_rank(token: &str, text: &str) -> Option<Rank> {
    if token.is_empty() {
        return Some(Rank::Prefix);
    }
    if text.starts_with(token) {
        return Some(Rank::Prefix);
    }
    let mut previous_alnum = false;
    for (index, c) in text.char_indices() {
        let alnum = c.is_alphanumeric();
        if alnum && !previous_alnum && index > 0 && text[index..].starts_with(token) {
            return Some(Rank::WordPrefix);
        }
        previous_alnum = alnum;
    }
    text.contains(token).then_some(Rank::Substring)
}

/// Indizes der Treffer: Präfixtreffer vor Wortanfängen vor Teiltreffern, innerhalb
/// einer Güte in der ursprünglichen Reihenfolge (stabil). Leere Suche liefert alle.
pub fn filter_ranked<T>(items: &[T], query: &str, text: impl Fn(&T) -> String) -> Vec<usize> {
    let matcher = Matcher::new(query);
    if matcher.is_empty() {
        return (0..items.len()).collect();
    }
    let mut hits: Vec<(Rank, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| matcher.rank(&text(item)).map(|rank| (rank, index)))
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, index)| index).collect()
}

/// Pfeiltasten in der Trefferliste: bleibt in `0..len` und läuft an den Enden um.
pub fn move_highlight(current: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as i64 + i64::from(delta);
    next.rem_euclid(len as i64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        [
            "Anna Müller",
            "Muriel Weber",
            "Hans Gruber",
            "Émile Mueller",
            "Karl-Heinz Mu",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn matching_ignores_case_and_umlaut_spelling() {
        let names = names();
        let hits = |q: &str| filter_ranked(&names, q, |n| n.clone());
        // `mü` findet Müller, Mueller und Mu, aber auch Muriel (Akzent entfernt); Präfix zuerst.
        assert_eq!(hits("MÜ"), [1, 0, 3, 4]);
        assert_eq!(hits("muel"), [0, 3]);
        assert_eq!(hits("émile"), [3]);
        assert_eq!(hits("emile"), [3]);
        assert!(hits("xyz").is_empty());
    }

    #[test]
    fn prefix_hits_come_before_word_starts_before_substrings_and_stay_stable() {
        let items = ["Marta Bauer", "Bauer, Anna", "Ambauer", "Bauern"].to_vec();
        let hits = filter_ranked(&items, "bauer", |s| s.to_string());
        // Präfix (1, 3) vor Wortanfang (0) vor Teiltreffer (2); gleiche Güte in Listenreihenfolge.
        assert_eq!(hits, [1, 3, 0, 2]);
    }

    #[test]
    fn every_word_must_match_and_the_first_word_sets_the_rank() {
        let items = ["Anna Maria Meier", "Maria Anna Meier", "Meier"].to_vec();
        assert_eq!(
            filter_ranked(&items, "maria meier", |s| s.to_string()),
            [1, 0]
        );
        assert_eq!(filter_ranked(&items, "  ", |s| s.to_string()), [0, 1, 2]);
    }

    #[test]
    fn identifiers_can_be_part_of_the_search_text() {
        let items = ["Anna #12", "Bernd #123", "Carl #7"].to_vec();
        assert_eq!(filter_ranked(&items, "#12", |s| s.to_string()), [0, 1]);
    }

    #[test]
    fn highlight_wraps_and_handles_empty_lists() {
        assert_eq!(move_highlight(0, 3, 1), 1);
        assert_eq!(move_highlight(2, 3, 1), 0);
        assert_eq!(move_highlight(0, 3, -1), 2);
        assert_eq!(move_highlight(5, 0, 1), 0);
    }

    #[test]
    fn sort_keys_put_accented_initials_with_their_base_letter() {
        assert!(sort_key("Ärztin") < sort_key("Berta"));
        assert_eq!(sort_key("ÉMILE"), sort_key("emile"));
    }
}
