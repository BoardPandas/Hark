//! Pure page packing for the book view: no `Ui`, so it is tested exhaustively.
//!
//! Position is an *anchor entry index*, never a page number. Resizing,
//! switching lettering, adding and deleting all reflow the pages; the spread
//! that holds the anchor stays open through every one of them.

use std::ops::Range;

/// Pack entries onto pages in order. An entry never splits across pages; an
/// entry taller than `capacity` gets a page to itself (its page scrolls).
/// `gap` separates entries on one page. Always returns at least one page, so
/// an empty book still has a page to open on.
pub fn paginate(heights: &[f32], capacity: f32, gap: f32) -> Vec<Range<usize>> {
    let mut pages = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (index, &height) in heights.iter().enumerate() {
        let needed = if index == start {
            height
        } else {
            used + gap + height
        };
        if index > start && needed > capacity {
            pages.push(start..index);
            start = index;
            used = height;
        } else {
            used = needed;
        }
    }
    pages.push(start..heights.len());
    pages
}

/// The page holding `anchor`; the last page if the anchor is past the end.
pub fn page_of(anchor: usize, pages: &[Range<usize>]) -> usize {
    pages
        .iter()
        .position(|page| page.contains(&anchor))
        .unwrap_or(pages.len().saturating_sub(1))
}

/// The spread (zero-based) holding `anchor`, with `per_spread` pages each.
pub fn spread_of(anchor: usize, pages: &[Range<usize>], per_spread: usize) -> usize {
    page_of(anchor, pages) / per_spread.max(1)
}

/// How many spreads `pages` fill.
pub fn spread_count(pages: &[Range<usize>], per_spread: usize) -> usize {
    pages.len().div_ceil(per_spread.max(1))
}

/// Keep the anchor on a real entry after a delete; 0 for an empty book.
pub fn clamp_anchor(anchor: usize, len: usize) -> usize {
    anchor.min(len.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_book_has_one_empty_page() {
        let empty = paginate(&[], 100.0, 4.0);
        assert_eq!(empty, vec![0..0]);
        assert_eq!(spread_count(&empty, 2), 1);
        assert_eq!(spread_of(0, &empty, 2), 0);
    }

    #[test]
    fn a_set_that_fits_is_one_page() {
        assert_eq!(paginate(&[30.0, 30.0, 30.0], 100.0, 5.0), vec![0..3]);
        // Exactly full still fits: 30 + 5 + 30 + 5 + 30 = 100.
        assert_eq!(paginate(&[30.0, 30.0, 30.0], 100.0, 5.0).len(), 1);
    }

    #[test]
    fn overflow_starts_a_new_page_and_counts_the_gap() {
        // 40 + 10 + 40 = 90 fits; + 10 + 40 = 140 does not.
        assert_eq!(
            paginate(&[40.0, 40.0, 40.0, 40.0], 100.0, 10.0),
            vec![0..2, 2..4]
        );
        // The gap alone pushes the second entry over.
        assert_eq!(paginate(&[50.0, 50.0], 100.0, 1.0), vec![0..1, 1..2]);
    }

    #[test]
    fn an_entry_taller_than_a_page_sits_alone() {
        assert_eq!(
            paginate(&[20.0, 250.0, 20.0, 20.0], 100.0, 5.0),
            vec![0..1, 1..2, 2..4]
        );
        assert_eq!(paginate(&[250.0], 100.0, 5.0), vec![0..1]);
        assert_eq!(paginate(&[250.0, 250.0], 100.0, 5.0), vec![0..1, 1..2]);
    }

    /// Every index lands on exactly one page, in order, with no empty page
    /// between real ones, and no page over capacity unless it holds one entry.
    #[test]
    fn every_entry_appears_exactly_once_in_order_over_random_sets() {
        // A fixed-seed LCG (Numerical Recipes constants): reproducible, no crate.
        let mut seed: u64 = 0x5eed_b00c;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) % (1 << 32);
            seed as f32 / (1u64 << 32) as f32
        };
        for _ in 0..200 {
            let len = (next() * 60.0) as usize;
            let capacity = 80.0 + next() * 400.0;
            let gap = next() * 20.0;
            let heights: Vec<f32> = (0..len).map(|_| 5.0 + next() * capacity * 1.3).collect();
            let pages = paginate(&heights, capacity, gap);

            let flat: Vec<usize> = pages.iter().flat_map(|page| page.clone()).collect();
            assert_eq!(flat, (0..len).collect::<Vec<_>>());
            for page in &pages {
                assert!(!page.is_empty() || len == 0, "empty page in {pages:?}");
                let used: f32 = heights[page.clone()].iter().sum::<f32>()
                    + gap * page.len().saturating_sub(1) as f32;
                assert!(
                    used <= capacity + 1e-3 || page.len() == 1,
                    "page {page:?} overflows: {used} > {capacity}"
                );
            }
            for anchor in 0..len {
                assert!(pages[page_of(anchor, &pages)].contains(&anchor));
            }
        }
    }

    #[test]
    fn spread_of_counts_one_or_two_pages_per_spread() {
        let pages = vec![0..2, 2..4, 4..5, 5..9, 9..10];
        assert_eq!(spread_of(0, &pages, 1), 0);
        assert_eq!(spread_of(3, &pages, 1), 1);
        assert_eq!(spread_of(9, &pages, 1), 4);
        assert_eq!(spread_of(0, &pages, 2), 0);
        assert_eq!(spread_of(3, &pages, 2), 0);
        assert_eq!(spread_of(4, &pages, 2), 1);
        assert_eq!(spread_of(9, &pages, 2), 2);
        assert_eq!(spread_count(&pages, 1), 5);
        assert_eq!(spread_count(&pages, 2), 3);
        // Past the end clamps to the last page.
        assert_eq!(spread_of(42, &pages, 2), 2);
    }

    #[test]
    fn clamp_anchor_survives_deleting_down_to_empty() {
        assert_eq!(clamp_anchor(5, 10), 5);
        assert_eq!(clamp_anchor(9, 9), 8);
        assert_eq!(clamp_anchor(3, 1), 0);
        assert_eq!(clamp_anchor(3, 0), 0);
    }
}
