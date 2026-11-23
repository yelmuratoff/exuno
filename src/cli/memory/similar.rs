//! How much two texts say the same thing: sets of three-word shingles,
//! compared by Jaccard similarity and by containment.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

/// Fewer shingles than this and a note is too short to compare fairly.
pub const MIN_SHINGLES: usize = 20;

/// The hashed three-word shingles of `text`: words are runs of letters and
/// digits in any script, lower-cased.
pub fn shingles(text: &str) -> HashSet<u64> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    words
        .windows(3)
        .map(|window| {
            let mut hasher = DefaultHasher::new();
            window.hash(&mut hasher);
            hasher.finish()
        })
        .collect()
}

/// Index pairs `(earlier, later, similarity)` of texts at or over `threshold`,
/// skipping texts with fewer than [`MIN_SHINGLES`] shingles.
pub fn similar_pairs(texts: &[HashSet<u64>], threshold: f64) -> Vec<(usize, usize, f64)> {
    let mut pairs = Vec::new();
    for (i, a) in texts.iter().enumerate() {
        if a.len() < MIN_SHINGLES {
            continue;
        }
        for (j, b) in texts.iter().enumerate().skip(i + 1) {
            if b.len() < MIN_SHINGLES {
                continue;
            }
            let score = jaccard(a, b);
            if score >= threshold {
                pairs.push((i, j, score));
            }
        }
    }
    pairs
}

/// The index and containment of the `wholes` entry that holds the most of
/// `part`, when that is at or over `threshold` and `part` is long enough.
pub fn best_container(
    part: &HashSet<u64>,
    wholes: &[HashSet<u64>],
    threshold: f64,
) -> Option<(usize, f64)> {
    if part.len() < MIN_SHINGLES {
        return None;
    }
    wholes
        .iter()
        .enumerate()
        .map(|(index, whole)| (index, containment(part, whole)))
        .filter(|(_, score)| *score >= threshold)
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Shared shingles over all shingles of the two texts.
pub fn jaccard(a: &HashSet<u64>, b: &HashSet<u64>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// The share of `part`'s shingles that `whole` also has.
pub fn containment(part: &HashSet<u64>, whole: &HashSet<u64>) -> f64 {
    if part.is_empty() {
        return 0.0;
    }
    part.intersection(whole).count() as f64 / part.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shingles_ignore_case_punctuation_and_script() {
        assert_eq!(
            shingles("One, two THREE four."),
            shingles("one two three four")
        );
        assert_eq!(shingles("один два три").len(), 1);
        assert!(shingles("one two").is_empty());
    }

    #[test]
    fn jaccard_and_containment_measure_overlap() {
        let a = shingles("a b c d e");
        let b = shingles("a b c d e f g");
        assert_eq!(jaccard(&a, &a), 1.0);
        assert_eq!(jaccard(&a, &b), 3.0 / 5.0);
        assert_eq!(containment(&a, &b), 1.0);
        assert_eq!(containment(&b, &a), 3.0 / 5.0);
        assert_eq!(jaccard(&HashSet::new(), &HashSet::new()), 0.0);
    }
}
