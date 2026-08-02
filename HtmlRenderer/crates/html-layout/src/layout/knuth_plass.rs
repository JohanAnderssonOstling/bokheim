//! Knuth–Plass optimal line breaking.
//!
//! Pure and self-contained: this module knows nothing about glyphs, styles, or
//! the layout tree. The inline layout code converts its token stream into
//! [`Item`]s, calls [`break_lines`], and maps the returned break indices back
//! onto tokens. Keeping it decoupled means it can be unit-tested in isolation
//! and swapped in behind a flag without disturbing the greedy breaker.
//!
//! The algorithm follows Knuth & Plass (1981): a paragraph is a stream of
//! boxes (unbreakable content), glue (stretchable/shrinkable space, the legal
//! break opportunities), and penalties (forced or optional breaks). A dynamic
//! program over "active" breakpoints finds the set of line breaks minimising
//! total demerits, where a line's demerits grow with how far its glue must
//! stretch or shrink from its natural width.

use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Box,
    Glue,
    Penalty,
}

/// One element of the paragraph stream.
pub struct Item {
    pub width: f64,
    pub stretch: f64,
    pub shrink: f64,
    pub penalty: f64,
    pub flagged: bool,
    pub kind: ItemKind,
}

/// Penalty values at or beyond these magnitudes are treated as "never break"
/// and "always break" respectively.
pub const INF_PENALTY: f64 = 10_000.0;
pub const FORCED_BREAK: f64 = -10_000.0;

/// Extra demerits charged when two consecutive lines differ sharply in
/// tightness (Knuth's adjacent-fitness penalty).
const FITNESS_PENALTY: f64 = 3000.0;

impl Item {
    pub fn box_(width: f64) -> Self {
        Self { width, stretch: 0.0, shrink: 0.0, penalty: 0.0, flagged: false, kind: ItemKind::Box }
    }

    pub fn glue(width: f64, stretch: f64, shrink: f64) -> Self {
        Self { width, stretch, shrink, penalty: 0.0, flagged: false, kind: ItemKind::Glue }
    }

    pub fn forced_break() -> Self {
        Self { width: 0.0, stretch: 0.0, shrink: 0.0, penalty: FORCED_BREAK, flagged: false, kind: ItemKind::Penalty }
    }

    fn is_forced(&self) -> bool {
        self.kind == ItemKind::Penalty && self.penalty <= FORCED_BREAK
    }
}

#[derive(Clone, Copy)]
struct Sums {
    width: f64,
    stretch: f64,
    shrink: f64,
}

/// A feasible breakpoint discovered by the DP. `previous` links back through
/// the chosen breaks so the optimal path can be reconstructed.
struct Node {
    position: usize,
    line: usize,
    fitness: i32,
    totals: Sums,
    demerits: f64,
    previous: Option<usize>,
}

/// Break `items` into lines whose widths are given by `line_width(line_index)`.
///
/// Returns the item indices at which the paragraph breaks (each is a glue or
/// penalty), in order, ending with the final forced break. Returns `None` when
/// no set of breaks keeps every line within tolerance — e.g. an unbreakable
/// box wider than its line — so callers can fall back to a greedy breaker
/// rather than render an overfull line.
pub fn break_lines(items: &[Item], line_width: impl Fn(usize) -> f64, tolerance: f64) -> Option<Vec<usize>> {
    if items.is_empty() {
        return Some(Vec::new());
    }

    let mut nodes: Vec<Node> = vec![Node { position: 0, line: 0, fitness: 1, totals: Sums { width: 0.0, stretch: 0.0, shrink: 0.0 }, demerits: 0.0, previous: None }];
    let mut active: Vec<usize> = vec![0];
    let mut sum = Sums { width: 0.0, stretch: 0.0, shrink: 0.0 };

    for b in 0..items.len() {
        let item = &items[b];
        let prev_is_box = b > 0 && items[b - 1].kind == ItemKind::Box;
        let is_break = match item.kind {
            ItemKind::Glue => prev_is_box,
            ItemKind::Penalty => item.penalty < INF_PENALTY,
            ItemKind::Box => false,
        };
        if is_break {
            consider_breaks(items, &line_width, tolerance, &mut nodes, &mut active, sum, b);
            if active.is_empty() {
                return None;
            }
        }
        match item.kind {
            ItemKind::Box => sum.width += item.width,
            ItemKind::Glue => {
                sum.width += item.width;
                sum.stretch += item.stretch;
                sum.shrink += item.shrink;
            }
            ItemKind::Penalty => {}
        }
    }

    // The surviving active nodes all sit at the final forced break; pick the
    // one with the fewest total demerits and walk its `previous` chain.
    let best = active.into_iter().min_by(|&x, &y| nodes[x].demerits.partial_cmp(&nodes[y].demerits).unwrap_or(Ordering::Equal))?;
    let mut breaks = Vec::new();
    let mut cur = Some(best);
    while let Some(n) = cur {
        if nodes[n].previous.is_some() {
            breaks.push(nodes[n].position);
        }
        cur = nodes[n].previous;
    }
    breaks.reverse();
    Some(breaks)
}

fn consider_breaks(items: &[Item], line_width: &impl Fn(usize) -> f64, tolerance: f64, nodes: &mut Vec<Node>, active: &mut Vec<usize>, sum: Sums, b: usize) {
    let item = &items[b];
    let forced = item.is_forced();
    let item_penalty = if item.kind == ItemKind::Penalty { item.penalty } else { 0.0 };

    // Track the best feasible candidate per fitness class so adjacent lines
    // don't swing between very tight and very loose.
    let mut best: [Option<(f64, usize)>; 4] = [None, None, None, None];

    let mut i = 0;
    while i < active.len() {
        let a = active[i];
        let avail = line_width(nodes[a].line);
        let r = adjustment_ratio(items, sum, nodes[a].totals, b, avail);

        if r >= -1.0 && r <= tolerance {
            let bad = 100.0 * r.abs().powi(3);
            let mut d = (1.0 + bad).powi(2);
            if item_penalty >= 0.0 && item_penalty < INF_PENALTY {
                d += item_penalty * item_penalty;
            } else if item_penalty < 0.0 && item_penalty > FORCED_BREAK {
                d -= item_penalty * item_penalty;
            }
            let fitness = fitness_class(r);
            if (fitness - nodes[a].fitness).abs() > 1 {
                d += FITNESS_PENALTY;
            }
            let total = nodes[a].demerits + d;
            let slot = fitness as usize;
            if best[slot].map_or(true, |(bd, _)| total < bd) {
                best[slot] = Some((total, a));
            }
        }

        // A node that overflows this line (r < -1) can never fit a later
        // breakpoint either, and a forced break retires every active node.
        if r < -1.0 || forced {
            active.remove(i);
        } else {
            i += 1;
        }
    }

    let totals_after = compute_sum(items, sum, b);
    for (fitness, candidate) in best.iter().enumerate() {
        if let Some((demerits, prev)) = *candidate {
            let idx = nodes.len();
            nodes.push(Node { position: b, line: nodes[prev].line + 1, fitness: fitness as i32, totals: totals_after, demerits, previous: Some(prev) });
            active.push(idx);
        }
    }
}

/// The stretch (positive) or shrink (negative) ratio needed to fit the line
/// from a node's breakpoint up to item `b` into `avail`.
fn adjustment_ratio(items: &[Item], sum: Sums, totals: Sums, b: usize, avail: f64) -> f64 {
    let mut w = sum.width - totals.width;
    if items[b].kind == ItemKind::Penalty {
        w += items[b].width;
    }
    if w < avail {
        let y = sum.stretch - totals.stretch;
        if y > 0.0 { (avail - w) / y } else { f64::INFINITY }
    } else if w > avail {
        let z = sum.shrink - totals.shrink;
        if z > 0.0 { (avail - w) / z } else { f64::NEG_INFINITY }
    } else {
        0.0
    }
}

/// Running totals just after breakpoint `b`, advancing over the discarded
/// trailing glue so the next line's measurements exclude it.
fn compute_sum(items: &[Item], base: Sums, b: usize) -> Sums {
    let mut s = base;
    let mut i = b;
    while i < items.len() {
        match items[i].kind {
            ItemKind::Glue => {
                s.width += items[i].width;
                s.stretch += items[i].stretch;
                s.shrink += items[i].shrink;
                i += 1;
            }
            ItemKind::Box => break,
            ItemKind::Penalty => {
                if items[i].is_forced() && i > b {
                    break;
                }
                i += 1;
            }
        }
    }
    s
}

fn fitness_class(r: f64) -> i32 {
    if r < -0.5 {
        0
    } else if r <= 0.5 {
        1
    } else if r <= 1.0 {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(words: &[f64], space: f64) -> Vec<Item> {
        let mut items = Vec::new();
        for (i, &w) in words.iter().enumerate() {
            if i > 0 {
                items.push(Item::glue(space, space * 0.5, space * 0.33));
            }
            items.push(Item::box_(w));
        }
        items.push(Item::glue(0.0, 1.0e9, 0.0));
        items.push(Item::forced_break());
        items
    }

    #[test]
    fn single_line_when_everything_fits() {
        let items = paragraph(&[30.0, 30.0, 30.0], 10.0);
        let breaks = break_lines(&items, |_| 1000.0, 8.0).unwrap();
        assert_eq!(breaks, vec![items.len() - 1], "only the final forced break");
    }

    #[test]
    fn breaks_when_narrow() {
        let items = paragraph(&[30.0, 30.0, 30.0], 10.0);
        // ~75px holds two words (70) but not three (110).
        let breaks = break_lines(&items, |_| 75.0, 8.0).unwrap();
        assert!(breaks.len() >= 2, "expected at least one mid-paragraph break, got {breaks:?}");
    }

    #[test]
    fn infeasible_when_word_wider_than_line() {
        let items = paragraph(&[200.0], 10.0);
        assert!(break_lines(&items, |_| 50.0, 8.0).is_none());
    }
}
