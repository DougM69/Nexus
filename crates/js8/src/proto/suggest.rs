//! JS8Call's word checker and its word suggestions (JSC_checker.cpp:113-345 and mainwindow.cpp:
//! 4918-4958, read as the spec; Settings ▸ General "Suggest alternative word choices for more
//! efficient message transmission", `Spellcheck`, on by default, Configuration.cpp:2408), over
//! the JSC dictionary ([`crate::proto::jsc`]).
//!
//! CONTRACT. [`needs_suggestion`] is JS8Call's test for the red wavy underline in its compose box.
//! A word, upper-cased, is left alone when it is shorter than four characters, all digits, a whole
//! dictionary word (the longest reachable dictionary prefix consumes all of it, `JSC::exists`), or
//! a callsign JS8Call's `isValidCallsign` accepts. Anything else is underlined: the dictionary
//! codes it in pieces, which costs airtime. [`suggestions`] is what JS8Call's right-click offers
//! for such a word: the longest reachable dictionary prefix when that consumes more than one
//! character, and every dictionary word one edit away (a letter A-Z added at either end or put in
//! place of one, or one character removed), cheapest first. Cheapest is the lowest dictionary
//! index, which is the shortest code on the air; the first `n` indices are offered. `found` says
//! whether any prefix was in the dictionary at all: that is how JS8Call tells "No Suggestions"
//! from offering nothing. A word that comes twice (the prefix is also one edit away, as "HELLO" is
//! for "HELLOX") is offered once.
//!
//! The desktop's compose box finds the words and asks about them (`ui/src/js8Suggest.ts`); the
//! dictionary stays here.
use crate::proto::callsign::{is_base_call, is_compound_call, split_portable, CallRef};
use crate::proto::jsc;

/// How many suggestions JS8Call offers (`JSCChecker::suggestions(word, 5, …)`).
pub const OFFERED: usize = 5;

const LETTERS: &[u8; 26] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// The dictionary's Latin-1 bytes for `text`: a character above U+00FF is '?', as
/// `QString::toLatin1` makes it.
fn latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            if (c as u32) <= 0xFF {
                c as u32 as u8
            } else {
                b'?'
            }
        })
        .collect()
}

fn from_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// `JSC::exists`: the dictionary index of `word` when the longest reachable prefix is all of it.
fn exists(word: &[u8]) -> Option<u32> {
    let d = jsc::dict();
    let idx = d.lookup(word)?;
    (d.consume(idx) == word.len()).then_some(idx)
}

/// JS8Call's `isValidCallsign` (varicode.cpp:1294-1324), as `compose::parse_directed` reads it: a
/// special (`<....>`, `@ALLCALL`, `@JS8NET`, a group), a base call with or without `/P`, or a valid
/// compound call.
pub fn is_valid_callsign(s: &str) -> bool {
    let special = CallRef::parse(s).is_some_and(|c| !matches!(c, CallRef::Base(_)));
    special || is_base_call(split_portable(s).0) || is_compound_call(s)
}

/// Whether JS8Call underlines `word` (`JSCChecker::checkRange`'s "correct" test, negated).
pub fn needs_suggestion(word: &str) -> bool {
    let w = word.trim().to_uppercase();
    if w.chars().count() < 4 || w.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    exists(&latin1(&w)).is_none() && !is_valid_callsign(&w)
}

/// What JS8Call's right-click offers for a word.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct Suggestions {
    /// Cheapest first.
    pub words: Vec<String>,
    /// Some prefix of the word is in the dictionary ("No Suggestions" when `words` is empty).
    pub found: bool,
}

/// Every string one edit from `word`: a letter added at the front or the back, a letter in place
/// of any character, or any character removed (`oneEdit`, JSC_checker.cpp:243-275).
fn one_edit(word: &[u8]) -> Vec<Vec<u8>> {
    let mut all = Vec::with_capacity(LETTERS.len() * (word.len() + 2) + word.len());
    for &l in LETTERS {
        let mut front = vec![l];
        front.extend_from_slice(word);
        all.push(front);
        let mut back = word.to_vec();
        back.push(l);
        all.push(back);
        for j in 0..word.len() {
            let mut swapped = word.to_vec();
            swapped[j] = l;
            all.push(swapped);
        }
    }
    for j in 0..word.len() {
        let mut cut = word.to_vec();
        cut.remove(j);
        all.push(cut);
    }
    all
}

/// The first `n` suggestions for `word`, cheapest first.
pub fn suggestions(word: &str, n: usize) -> Suggestions {
    let w = latin1(&word.trim().to_uppercase());
    let d = jsc::dict();
    let prefix = d.lookup(&w);
    let mut offers: Vec<(u32, Vec<u8>)> = Vec::new();
    if let Some(idx) = prefix {
        let size = d.consume(idx).min(d.word(idx).len());
        if size > 1 {
            offers.push((idx, d.word(idx)[..size].to_vec()));
        }
    }
    offers.extend(
        one_edit(&w)
            .into_iter()
            .filter_map(|c| exists(&c).map(|idx| (idx, c))),
    );
    offers.sort_by_key(|(idx, _)| *idx);
    let mut words: Vec<String> = Vec::new();
    let mut indices = 0;
    let mut last = None;
    for (idx, bytes) in offers {
        if last != Some(idx) {
            if indices == n {
                break;
            }
            indices += 1;
            last = Some(idx);
        }
        let s = from_latin1(&bytes);
        if !words.contains(&s) {
            words.push(s);
        }
    }
    Suggestions {
        words,
        found: prefix.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An independent oracle for one word: every dictionary entry, scanned, kept when it is a
    /// whole reachable word one edit from `w` — by the edit-distance definition, not by
    /// generating candidates — plus the prefix rule; sorted by index.
    fn oracle(word: &str, n: usize) -> Vec<String> {
        let w = latin1(&word.to_uppercase());
        let d = jsc::dict();
        let one_apart = |c: &[u8]| -> bool {
            let (a, b) = (c, w.as_slice());
            if a.len() == b.len() {
                a.iter().zip(b).filter(|(x, y)| x != y).count() == 1
                    && a.iter().zip(b).all(|(x, y)| x == y || LETTERS.contains(x))
            } else if a.len() + 1 == b.len() {
                (0..b.len()).any(|j| [&b[..j], &b[j + 1..]].concat() == a)
            } else if a.len() == b.len() + 1 {
                (LETTERS.contains(&a[0]) && &a[1..] == b)
                    || (LETTERS.contains(&a[a.len() - 1]) && &a[..a.len() - 1] == b)
            } else {
                false
            }
        };
        let mut found: Vec<(u32, String)> = (0..jsc::SIZE as u32)
            .filter(|&i| {
                let wd = d.word(i);
                one_apart(wd) && exists(wd) == Some(i)
            })
            .map(|i| (i, from_latin1(d.word(i))))
            .collect();
        if let Some(idx) = d.lookup(&w) {
            let size = d.consume(idx).min(d.word(idx).len());
            if size > 1 {
                found.push((idx, from_latin1(&d.word(idx)[..size])));
            }
        }
        found.sort();
        found.dedup();
        let mut keys: Vec<u32> = found.iter().map(|(i, _)| *i).collect();
        keys.dedup();
        keys.truncate(n);
        found
            .into_iter()
            .filter(|(i, _)| keys.contains(i))
            .map(|(_, s)| s)
            .collect()
    }

    #[test]
    fn short_numeric_whole_words_and_callsigns_are_left_alone() {
        for w in [
            "TNX", "GM", "73", "1234", "14078", "KD9TAW", "W1AW", "@ALLCALL", "W1AW/P",
        ] {
            assert!(!needs_suggestion(w), "{w} is not underlined");
        }
        // A word the dictionary holds whole, checked against the dictionary itself.
        for w in ["HELLO", "THANKS", "WEATHER"] {
            assert!(
                exists(w.as_bytes()).is_some(),
                "control: {w} is a dictionary word"
            );
            assert!(!needs_suggestion(w), "{w} is not underlined");
            assert!(
                !needs_suggestion(&w.to_lowercase()),
                "{w} in lower case either"
            );
        }
    }

    #[test]
    fn a_word_the_dictionary_codes_in_pieces_is_underlined() {
        for w in ["HELLOX", "XQZJV", "WEATHR"] {
            assert!(
                exists(w.as_bytes()).is_none(),
                "control: {w} is not a whole word"
            );
            assert!(needs_suggestion(w), "{w} is underlined");
        }
    }

    #[test]
    fn suggestions_agree_with_a_full_dictionary_scan() {
        for w in [
            "HELLOX", "WEATHR", "ANTENNAA", "QRZZ", "QXZJ", "JJJJJ", "XQZJV",
        ] {
            let got = suggestions(w, OFFERED);
            assert_eq!(got.words, oracle(w, OFFERED), "{w}");
            assert!(got.words.len() <= OFFERED, "{w}: at most {OFFERED}");
        }
    }

    #[test]
    fn the_cheapest_come_first_and_the_prefix_is_offered_once() {
        let s = suggestions("HELLOX", OFFERED);
        assert!(s.found);
        assert_eq!(s.words, ["HELLO", "HELLOS", "HELLOW"]);
        assert_eq!(
            s.words.iter().filter(|w| *w == "HELLO").count(),
            1,
            "HELLO is both the prefix and one edit away, and is offered once: {s:?}"
        );
        let idx: Vec<u32> = s
            .words
            .iter()
            .map(|w| exists(w.as_bytes()).expect("each offer is a whole word"))
            .collect();
        let mut sorted = idx.clone();
        sorted.sort();
        assert_eq!(idx, sorted, "lowest index (shortest code) first");
    }

    #[test]
    fn a_word_with_no_dictionary_prefix_offers_nothing_and_says_so() {
        // '§' starts no dictionary word, and no single edit makes a word of "§§§§§".
        let s = suggestions("§§§§§", OFFERED);
        assert_eq!(s, Suggestions::default());
        assert!(needs_suggestion("§§§§§"), "and it is underlined");
    }
}
