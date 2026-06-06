//! Word-level (intra-line) diffing. Given a paired removed/added line, computes
//! the changed byte ranges on each side, using imara-diff at word-token
//! granularity (same engine as the line diff, different tokens — ADR 0002).

use super::Segment;
use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};

/// Character class for tokenization. Runs of the same class form one token,
/// except punctuation/other which is emitted per-character for fine granularity.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Other,
}

fn class_of(c: char) -> Class {
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Other
    }
}

/// Tokenize a line into byte ranges: maximal runs of word/space characters, and
/// single-character tokens for everything else.
fn word_ranges(s: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let cl = class_of(c);
        let mut end = i + c.len_utf8();
        if cl != Class::Other {
            while let Some(&(j, c2)) = chars.peek() {
                if class_of(c2) == cl {
                    end = j + c2.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
        }
        ranges.push((i, end));
    }
    ranges
}

/// A [`TokenSource`] over the word tokens of a line.
struct Words<'a>(&'a str);

impl<'a> TokenSource for Words<'a> {
    type Token = &'a str;
    type Tokenizer = WordsIter<'a>;

    fn tokenize(&self) -> Self::Tokenizer {
        WordsIter {
            s: self.0,
            ranges: word_ranges(self.0).into_iter(),
        }
    }

    fn estimate_tokens(&self) -> u32 {
        (self.0.len() / 4 + 1) as u32
    }
}

struct WordsIter<'a> {
    s: &'a str,
    ranges: std::vec::IntoIter<(usize, usize)>,
}

impl<'a> Iterator for WordsIter<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
        self.ranges.next().map(|(a, b)| &self.s[a..b])
    }
}

/// Changed byte ranges within an old line and a new line. Returns
/// `(old_segments, new_segments)`, each marking the spans that differ.
pub fn segments(old: &str, new: &str) -> (Vec<Segment>, Vec<Segment>) {
    let old_ranges = word_ranges(old);
    let new_ranges = word_ranges(new);

    let input = InternedInput::new(Words(old), Words(new));
    let diff = Diff::compute(Algorithm::Histogram, &input);

    let mut old_segs = Vec::new();
    let mut new_segs = Vec::new();
    for hunk in diff.hunks() {
        if !hunk.before.is_empty() {
            let start = old_ranges[hunk.before.start as usize].0;
            let end = old_ranges[hunk.before.end as usize - 1].1;
            old_segs.push(Segment {
                start,
                end,
                changed: true,
            });
        }
        if !hunk.after.is_empty() {
            let start = new_ranges[hunk.after.start as usize].0;
            let end = new_ranges[hunk.after.end as usize - 1].1;
            new_segs.push(Segment {
                start,
                end,
                changed: true,
            });
        }
    }
    (old_segs, new_segs)
}
