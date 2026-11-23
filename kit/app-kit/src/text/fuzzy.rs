//! The one forgiving search every "find a thing" in the app uses: a
//! session, a folder, a target to send to. Each word of the query is found
//! in order in a thing's text, letters apart if need be (`dbm` finds
//! `db-migrate`), case aside; the words in any order. A word that is not
//! there as typed is still found with one letter left out of it, so a
//! slip (`mirgate`, `harbuor`) costs a little rank, not the match.
//!
//! A match scores as CHROTE's does: a letter at the start of a word or
//! after a separator counts most, letters that follow each other count
//! more than letters apart. What matched is returned with the score, so a
//! list can mark the letters that found it.
//!
//! A page's list filters through [`filter`] (or [`matches`] for one
//! thing): what was typed as it stands ranks above letters apart.

/// How a text matched: its score and the characters (by index in the
/// text) that matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub score: i64,
    pub at: Vec<usize>,
}

/// A letter as the search compares it, case aside.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Whether the character at `i` starts a word: the first, one after a
/// character that is not a letter or a digit, or an upper-case letter
/// after a lower-case one.
fn starts(text: &[char], i: usize) -> bool {
    i == 0
        || !text[i - 1].is_alphanumeric()
        || (text[i - 1].is_lowercase() && text[i].is_uppercase())
}

/// The best way `word` is found in order in `text`, if it is.
fn exact(word: &[char], text: &[char]) -> Option<Hit> {
    if word.is_empty() || word.len() > text.len() {
        return None;
    }
    let lowered: Vec<char> = text.iter().map(|c| fold(*c)).collect();
    // best[i]: the best score with the word's letters so far ending at i,
    // and where the letter before ended.
    let mut best: Vec<Option<(i64, usize)>> = vec![None; text.len()];
    let mut trail: Vec<Vec<Option<(i64, usize)>>> = Vec::with_capacity(word.len());
    for (n, letter) in word.iter().map(|c| fold(*c)).enumerate() {
        let mut next = vec![None; text.len()];
        // The best of `best` before each place, with its place.
        let mut before: Option<(i64, usize)> = None;
        for i in 0..text.len() {
            if lowered[i] == letter {
                let base = if starts(text, i) { 8 } else { 2 };
                next[i] = if n == 0 {
                    // Earlier is better, a little: a name read from its start.
                    Some((base - (i as i64).min(8) / 2, usize::MAX))
                } else {
                    let follows = (i > 0)
                        .then(|| best[i - 1])
                        .flatten()
                        .map(|(s, _)| (s + base + 6, i - 1));
                    let apart = before.map(|(s, j)| (s + base - 3 * (i - j - 1).min(6) as i64, j));
                    match (follows, apart) {
                        (Some(a), Some(b)) => Some(if a.0 >= b.0 { a } else { b }),
                        (a, b) => a.or(b),
                    }
                };
            }
            if let Some((s, _)) = best[i]
                && before.is_none_or(|(b, _)| s > b)
            {
                before = Some((s, i));
            }
        }
        trail.push(next.clone());
        best = next;
    }
    let (end, &(score, _)) = best
        .iter()
        .enumerate()
        .filter_map(|(i, b)| b.as_ref().map(|b| (i, b)))
        .max_by_key(|(i, (s, _))| (*s, std::cmp::Reverse(*i)))?;
    let mut at = vec![end];
    let mut i = end;
    for n in (1..word.len()).rev() {
        let (_, j) = trail[n][i]?;
        at.push(j);
        i = j;
    }
    at.reverse();
    Some(Hit { score, at })
}

/// How `word` is found in `text`: as typed, else with one of its letters
/// left out (a slip), for a word long enough that this still says much.
pub fn word(word: &str, text: &str) -> Option<Hit> {
    let w: Vec<char> = word.chars().collect();
    let t: Vec<char> = text.chars().collect();
    if let Some(hit) = exact(&w, &t) {
        return Some(hit);
    }
    if w.len() < 4 {
        return None;
    }
    (0..w.len())
        .filter_map(|skip| {
            let fewer: Vec<char> = w
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != skip)
                .map(|(_, c)| *c)
                .collect();
            exact(&fewer, &t)
        })
        .max_by_key(|h| h.score)
        .map(|h| Hit {
            score: h.score - 12,
            ..h
        })
}

/// How `query` matches `text`: every word of it found, the scores added
/// and the matched characters together. An empty query matches all,
/// with no score.
pub fn find(query: &str, text: &str) -> Option<Hit> {
    let mut all = Hit {
        score: 0,
        at: vec![],
    };
    for w in query.split_whitespace() {
        let hit = word(w, text)?;
        all.score += hit.score;
        all.at.extend(hit.at);
    }
    all.at.sort_unstable();
    all.at.dedup();
    Some(all)
}

/// How `query` matches a thing known by several fields: its name first,
/// then other words for it (its program, its folder, its device). Each word
/// of the query is found within one field, never across two, and counts a
/// little less when only another field has it, so a name that matches
/// ranks first. The matched characters are the name's.
pub fn find_fields(query: &str, fields: &[&str]) -> Option<Hit> {
    find_each_field(query, fields).map(|h| Hit {
        score: h.score,
        at: h.at.into_iter().next().unwrap_or_default(),
    })
}

/// How a thing known by several fields matched: its score as
/// [`find_fields`] gives it, and the matched characters of each field, in
/// the fields' order, so a row found by its folder can mark the folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldsHit {
    pub score: i64,
    pub at: Vec<Vec<usize>>,
}

/// [`find_fields`], with each word's letters marked in the field that
/// found it.
pub fn find_each_field(query: &str, fields: &[&str]) -> Option<FieldsHit> {
    let (name, rest) = fields.split_first()?;
    let mut all = FieldsHit {
        score: 0,
        at: vec![vec![]; fields.len()],
    };
    for w in query.split_whitespace() {
        let in_name = word(w, name);
        let elsewhere = rest
            .iter()
            .enumerate()
            .filter_map(|(i, f)| word(w, f).map(|h| (i + 1, h)))
            .map(|(i, h)| (i, h.score - 4, h.at))
            // The best, the first field of those as good.
            .rev()
            .max_by_key(|(_, score, _)| *score);
        let (field, score, at) = match (in_name, elsewhere) {
            (Some(hit), Some(other)) if other.1 > hit.score => other,
            (Some(hit), _) => (0, hit.score, hit.at),
            (None, Some(other)) => other,
            (None, None) => return None,
        };
        all.score += score;
        all.at[field].extend(at);
    }
    for at in &mut all.at {
        at.sort_unstable();
        at.dedup();
    }
    Some(all)
}

/// The places of `texts` that match `query`, best first, ties in their
/// given order: each with its index in `texts` and its [`Hit`].
pub fn rank<'a>(query: &str, texts: impl IntoIterator<Item = &'a str>) -> Vec<(usize, Hit)> {
    let mut found: Vec<(usize, Hit)> = texts
        .into_iter()
        .enumerate()
        .filter_map(|(i, t)| find(query, t).map(|h| (i, h)))
        .collect();
    found.sort_by_key(|(i, h)| (std::cmp::Reverse(h.score), *i));
    found
}

/// How a thing a page filters by its fields matched: its score and the
/// matched characters of each field, in the fields' order, for marking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Where the thing stands in the list filtered.
    pub index: usize,
    pub score: i64,
    pub at: Vec<Vec<usize>>,
}

/// How `word` is found close enough in `field` to say something, so a
/// word does not match letters strewn across a long description: as typed
/// with its letters within twice its length, else with a slipped letter
/// left out and the rest together in one word, one gap at most.
fn near(word: &str, field: &str) -> Option<Hit> {
    let span = |hit: &Hit| hit.at.last().map_or(0, |l| l + 1 - hit.at[0]);
    let w: Vec<char> = word.chars().collect();
    let t: Vec<char> = field.chars().collect();
    if let Some(hit) = exact(&w, &t).filter(|h| span(h) <= 2 * w.len() + 2) {
        return Some(hit);
    }
    if w.len() < 4 {
        return None;
    }
    (0..w.len())
        .filter_map(|skip| {
            let mut fewer = w.clone();
            fewer.remove(skip);
            exact(&fewer, &t).filter(|h| {
                span(h) <= fewer.len() + 1
                    && !t[h.at[0]..=h.at[h.at.len() - 1]]
                        .iter()
                        .any(|c| c.is_whitespace())
            })
        })
        .max_by_key(|h| h.score)
        .map(|h| Hit {
            score: h.score - 12,
            ..h
        })
}

/// How `query` matches a thing a page lists by its `fields`, its name
/// first: each word found in one field (a slipped letter forgiven), and
/// the whole query as typed counting most, in the name above all. `None`
/// when a word is not there; an empty query matches with no score.
pub fn matches<S: AsRef<str>>(query: &str, fields: &[S]) -> Option<FieldsHit> {
    let mut all = FieldsHit {
        score: 0,
        at: vec![vec![]; fields.len()],
    };
    for w in query.split_whitespace() {
        let (field, score, at) = fields
            .iter()
            .enumerate()
            .filter_map(|(i, f)| near(w, f.as_ref()).map(|h| (i, h)))
            .map(|(i, h)| (i, h.score - if i == 0 { 0 } else { 4 }, h.at))
            // The best, the first field of those as good.
            .rev()
            .max_by_key(|(_, score, _)| *score)?;
        all.score += score;
        all.at[field].extend(at);
    }
    for at in &mut all.at {
        at.sort_unstable();
        at.dedup();
    }
    let typed = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if !typed.is_empty() {
        let lowered: Vec<String> = fields.iter().map(|f| f.as_ref().to_lowercase()).collect();
        all.score += match lowered.split_first() {
            Some((name, _)) if *name == typed => 300,
            Some((name, _)) if name.starts_with(&typed) => 200,
            Some((name, _)) if name.contains(&typed) => 100,
            Some((_, rest)) if rest.iter().any(|f| f.contains(&typed)) => 50,
            _ => 0,
        };
    }
    Some(all)
}

/// The things of `items` that `query` matches by the `fields` each is
/// known by (its name first), best first: what was typed as it stands
/// before letters apart or a slip, ties in the list's order. An empty
/// query keeps them all, in order.
pub fn filter<T, S: AsRef<str>>(
    query: &str,
    items: &[T],
    fields: impl Fn(&T) -> Vec<S>,
) -> Vec<Found> {
    let mut found: Vec<Found> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            matches(query, &fields(item)).map(|h| Found {
                index,
                score: h.score,
                at: h.at,
            })
        })
        .collect();
    found.sort_by_key(|f| (std::cmp::Reverse(f.score), f.index));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(query: &str, texts: &[&str]) -> Vec<String> {
        rank(query, texts.iter().copied())
            .into_iter()
            .map(|(i, _)| texts[i].to_string())
            .collect()
    }

    #[test]
    fn letters_apart_find_a_name_and_word_starts_rank_first() {
        let names = ["api-tests", "build-runner", "db-migrate", "docs-writer"];
        assert_eq!(order("dbm", &names), ["db-migrate"]);
        assert_eq!(order("dw", &names), ["docs-writer"]);
        // A run at the start of words beats the same letters scattered.
        assert_eq!(
            order("run", &["harbour-ops-unit", "build-runner"]),
            ["build-runner", "harbour-ops-unit"]
        );
        assert_eq!(order("TESTS", &names), ["api-tests"]);
    }

    #[test]
    fn the_words_are_found_in_any_order() {
        let names = ["claude harbour", "codex harbour", "claude docs"];
        assert_eq!(order("harbour claude", &names), ["claude harbour"]);
        assert!(order("harbour zebra", &names).is_empty());
    }

    #[test]
    fn a_slip_of_one_letter_still_finds_it_after_what_was_typed() {
        let names = ["db-migrate", "harbour-ops", "migration-notes"];
        assert_eq!(order("mirgate", &names)[0], "db-migrate");
        assert_eq!(order("harbuor", &names), ["harbour-ops"]);
        // Typed as it is, it ranks above a slip.
        let a = find("migrat", "migration-notes").unwrap().score;
        let b = find("mirgat", "db-migrate").unwrap().score;
        assert!(a > b);
        // Short words do not forgive: too much would match.
        assert!(find("xb", "db-migrate").is_none());
    }

    #[test]
    fn a_word_is_found_within_one_field_and_the_name_ranks_first() {
        // "cx" is not in either field, though its letters are in both.
        assert!(find_fields("cx", &["claude", "box"]).is_none());
        assert!(find_fields("codex harbour", &["harbour-ops", "codex"]).is_some());
        let named = find_fields("docs", &["docs-writer", "claude"]).unwrap();
        let elsewhere = find_fields("docs", &["build-runner", "docs"]).unwrap();
        assert!(named.score > elsewhere.score);
        // Only the name's letters are marked.
        assert_eq!(elsewhere.at, Vec::<usize>::new());
        assert_eq!(named.at, vec![0, 1, 2, 3]);
    }

    #[test]
    fn each_word_marks_the_field_that_found_it() {
        // A row found by its folder marks the folder, not the name.
        let fields = ["api-tests", "claude", "harbour"];
        let hit = find_each_field("hrbr", &fields).unwrap();
        assert_eq!(hit.at, vec![vec![], vec![], vec![0, 2, 3, 6]]);
        assert_eq!(hit.score, find_fields("hrbr", &fields).unwrap().score);
        // Two words, two fields: the name and the program.
        let hit = find_each_field("api cla", &fields).unwrap();
        assert_eq!(hit.at, vec![vec![0, 1, 2], vec![0, 1, 2], vec![]]);
        // Where the name has it, the name is marked.
        let hit = find_each_field("tests", &["api-tests", "tests"]).unwrap();
        assert_eq!(hit.at, vec![vec![4, 5, 6, 7, 8], vec![]]);
        // Of two other fields as good, the first is marked.
        let hit = find_each_field("docs", &["x", "docs", "docs"]).unwrap();
        assert_eq!(hit.at, vec![vec![], vec![0, 1, 2, 3], vec![]]);
    }

    #[test]
    fn the_matched_letters_are_returned_to_be_marked() {
        assert_eq!(find("dbm", "db-migrate").unwrap().at, vec![0, 1, 3]);
        assert_eq!(find("hop", "harbour-ops").unwrap().at, vec![0, 8, 9]);
        assert_eq!(find("", "anything").unwrap().at, Vec::<usize>::new());
        // Two words that use the same letter mark it once.
        assert_eq!(find("ha ha", "harbour").unwrap().at, vec![0, 1]);
    }

    fn filtered(query: &str, items: &[(&str, &str)]) -> Vec<String> {
        filter(query, items, |(name, about)| vec![*name, *about])
            .into_iter()
            .map(|f| items[f.index].0.to_string())
            .collect()
    }

    #[test]
    fn a_filter_puts_what_was_typed_before_letters_apart_and_slips() {
        let items = [
            ("my-image-gen", ""),
            ("migration", ""),
            ("db-migrate", "moves the schema"),
            ("notes", "about migrate"),
        ];
        // A prefix beats the same letters apart.
        assert_eq!(filtered("mig", &items)[0], "migration");
        // As typed in the name, then in another field.
        assert_eq!(
            filtered("migrate", &items),
            ["db-migrate", "notes", "migration"]
        );
        // A slipped letter still finds it.
        assert_eq!(filtered("mirgate", &items)[0], "db-migrate");
        // Letters strewn across a description are not a match, nor is a
        // slip with what is left of the word strewn.
        assert!(filtered("zoom", &[("x", "zebras on our map")]).is_empty());
        assert_eq!(
            filtered(
                "thmee",
                &[
                    ("Node", "The node this app talks to"),
                    ("Panel", "The edge"),
                    ("Theme", "")
                ]
            ),
            ["Theme"]
        );
        // Nothing typed keeps them all, in order.
        assert_eq!(filtered(" ", &items).len(), 4);
        let marked = filter("schema", &items, |(n, a)| vec![*n, *a]);
        assert_eq!(marked[0].at, vec![vec![], (10..16).collect::<Vec<_>>()]);
    }
}
