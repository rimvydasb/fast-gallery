//! Clusters and orders of signatures. All functions are pure: they get signatures (and times) in
//! the caller's order and return indexes into that order.

use serde::{Deserialize, Serialize};

use super::Signature;

/// A signature in the form for many comparisons: the color layout is centered and normalized, so
/// that the correlation of 2 layouts is a dot product.
struct Prepared<'a> {
    sig: &'a Signature,
    layout: Vec<f32>,
    /// The layout has no contrast (for example a blank page).
    flat: bool,
}

impl<'a> Prepared<'a> {
    fn new(sig: &'a Signature) -> Self {
        let n = sig.c.len().max(1) as f32;
        let mean = sig.c.iter().map(|&v| v as f32).sum::<f32>() / n;
        let mut layout: Vec<f32> = sig.c.iter().map(|&v| v as f32 - mean).collect();
        let norm = layout.iter().map(|v| v * v).sum::<f32>().sqrt();
        // A standard deviation below 2 gray levels.
        let flat = norm < 2.0 * n.sqrt();
        if !flat {
            layout.iter_mut().for_each(|v| *v /= norm);
        }
        Self { sig, layout, flat }
    }

    /// Similarity of 2 photos, 0 to 1 (equal). 3 parts:
    /// - structure: the hash distance, 1 at 0 bits and 0 from 64 bits (unrelated photos);
    /// - layout: the correlation of the 4 × 4 color layouts, which a small camera move changes little;
    /// - palette: the common share of the color histograms, which a camera move does not change.
    ///
    /// Layout × palette has the weight 0.75: photos of a hand-held series have nearly the same
    /// palette and layout, but hashes that differ like the hashes of unrelated photos. The
    /// product needs both: pages of text share the palette and not the layout.
    fn similarity(&self, o: &Prepared) -> f32 {
        let structure = (1.0 - self.sig.hash_dist(o.sig) as f32 / 64.0).max(0.0);
        let layout = match (self.flat, o.flat) {
            (true, true) => 1.0,
            (false, false) => self.layout.iter().zip(&o.layout).map(|(a, b)| a * b).sum::<f32>().max(0.0),
            _ => 0.0,
        };
        let palette = 1.0 - self.sig.hist_dist(o.sig) as f32 / 255.0;
        0.25 * structure + 0.75 * layout * palette
    }
}

/// How similar 2 photos must be to be in 1 group.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// The same photo in another format, size, or compression.
    Same,
    High,
    Medium,
    Low,
}

impl Level {
    /// The minimum similarity.
    /// Assumption: first estimates from the example gallery (`the_example_burst_is_found`).
    pub fn threshold(self) -> f32 {
        match self {
            Level::Same => 0.90,
            Level::High => 0.70,
            Level::Medium => 0.55,
            Level::Low => 0.40,
        }
    }

    fn matches(self, a: &Prepared, b: &Prepared) -> bool {
        // The same photo also has nearly the same hashes.
        (self != Level::Same || a.sig.hash_dist(b.sig) <= 8) && a.similarity(b) >= self.threshold()
    }
}

fn prepare<'a>(sigs: impl Iterator<Item = Option<&'a Signature>>) -> Vec<Option<Prepared<'a>>> {
    sigs.map(|s| s.map(Prepared::new)).collect()
}

/// How far below the level a photo may be from the first photo of its group.
const LEADER_SLACK: f32 = 0.15;

/// Groups of similar photos in the whole list, in any order and at any distance in time. A photo
/// joins the first group that has a member that matches it at `level`, if it is also near the
/// first photo of the group (the leader): at most `LEADER_SLACK` below the level. The leader rule
/// keeps a group from drifting over a long chain of small differences. Else the photo starts a
/// group. Returns the groups with 2 or more photos, as indexes, in order of their leader.
pub fn cluster_similar(sigs: &[Option<&Signature>], level: Level) -> Vec<Vec<usize>> {
    let prep = prepare(sigs.iter().copied());
    let near = level.threshold() - LEADER_SLACK;
    let get = |i: usize| prep[i].as_ref();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, s) in prep.iter().enumerate() {
        let Some(s) = s else { continue };
        let home = groups.iter_mut().find(|g| {
            get(g[0]).is_some_and(|l| l.similarity(s) >= near)
                && g.iter().any(|&m| get(m).is_some_and(|o| level.matches(o, s)))
        });
        match home {
            Some(g) => g.push(i),
            None => groups.push(vec![i]),
        }
    }
    groups.retain(|g| g.len() > 1);
    groups
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurstKind {
    /// A series taken at a high frequency: the time gap must fit the rhythm of the series.
    Dense,
    /// Similar photos that follow one after another, with no time limit.
    Sparse,
}

/// Time gap limit of a dense burst before its rhythm is known, s.
const BURST_FIRST_GAP: i64 = 3;
/// The gap limit follows the mean gap of the burst, between these bounds, s.
const BURST_MIN_GAP: i64 = 2;
const BURST_MAX_GAP: i64 = 10;

/// Bursts in a list that is sorted by time. `items` = (time in s, signature).
/// A photo continues the burst if it is similar to the previous photo or to the first photo of the
/// burst, and, for `Dense`, if its time gap is within the limit. The limit starts at 3 s; after 2
/// gaps it is 2.5 × the mean gap of the burst + 1 s, between 2 s and 10 s (EXIF times have a
/// resolution of 1 s). `Dense` accepts a lower similarity (`Level::Low`) than `Sparse`
/// (`Level::Medium`), because the time gap is a second proof. Returns the bursts with 2 or more
/// photos, as indexes.
pub fn cluster_bursts(items: &[(i64, Option<&Signature>)], kind: BurstKind) -> Vec<Vec<usize>> {
    let level = match kind {
        BurstKind::Dense => Level::Low,
        BurstKind::Sparse => Level::Medium,
    };
    let prep = prepare(items.iter().map(|(_, s)| *s));
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    for (i, s) in prep.iter().enumerate() {
        let t = items[i].0;
        let joins = match (cur.last(), s) {
            (Some(&prev), Some(s)) => {
                let similar = [prev, cur[0]].iter().any(|&j| prep[j].as_ref().is_some_and(|o| level.matches(o, s)));
                let in_time = kind == BurstKind::Sparse || {
                    let gaps = (cur.len() - 1) as i64;
                    let limit = if gaps < 2 {
                        BURST_FIRST_GAP
                    } else {
                        let span = items[prev].0 - items[cur[0]].0;
                        (span * 5 / (gaps * 2) + 1).clamp(BURST_MIN_GAP, BURST_MAX_GAP)
                    };
                    (t - items[prev].0).abs() <= limit
                };
                similar && in_time
            }
            _ => false,
        };
        if !joins {
            if cur.len() > 1 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
        }
        if s.is_some() {
            cur.push(i);
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// An order in which similar photos are neighbors: a greedy chain that starts at the first photo
/// and always continues with the most similar photo that is left. O(n²) comparisons: about 2 s for
/// 10,000 photos.
pub fn similarity_order(sigs: &[&Signature]) -> Vec<usize> {
    let n = sigs.len();
    let mut order = Vec::with_capacity(n);
    if n == 0 {
        return order;
    }
    let prep: Vec<Prepared> = sigs.iter().map(|s| Prepared::new(s)).collect();
    let mut left: Vec<usize> = (1..n).collect();
    let mut cur = 0usize;
    order.push(cur);
    while !left.is_empty() {
        let mut best = 0usize;
        let mut best_s = f32::MIN;
        // `>` keeps the earlier photo among equally similar photos.
        for (k, &j) in left.iter().enumerate() {
            let s = prep[cur].similarity(&prep[j]);
            if s > best_s {
                best_s = s;
                best = k;
            }
        }
        cur = left.remove(best);
        order.push(cur);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A signature of scene `id`. `bits` hash bits differ from the scene (64: like an unrelated
    /// photo), and `shift` of 255 of the palette is in another color.
    /// Same scene: similarity = 0.25 × (1 − bits / 64) + 0.75 × (1 − shift / 255).
    fn sig(id: u8, bits: u32, shift: u8) -> Signature {
        let base = (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let flip = if bits == 0 { 0 } else { u64::MAX >> (64 - bits.min(64)) };
        let c = (0..48u32).map(|i| ((i * 37 + id as u32 * 91) * 7 % 200) as u8).collect();
        let mut h = vec![0u8; 64];
        h[id as usize % 60] = 255 - shift;
        h[id as usize % 60 + 1] = shift;
        Signature { p: base ^ flip, d: base.rotate_left(17), c, h }
    }

    fn all(v: &[Signature]) -> Vec<Option<&Signature>> {
        v.iter().map(Some).collect()
    }

    #[test]
    fn levels_are_nested() {
        let a = sig(1, 0, 0);
        let groups = |other: Signature, level| cluster_similar(&all(&[a.clone(), other]), level).len();
        // 0.98: the same photo.
        assert_eq!(groups(sig(1, 4, 0), Level::Same), 1);
        // 0.80: High, not Same.
        assert_eq!(groups(sig(1, 32, 26), Level::Same), 0);
        assert_eq!(groups(sig(1, 32, 26), Level::High), 1);
        // 0.60: Medium, not High.
        assert_eq!(groups(sig(1, 64, 51), Level::High), 0);
        assert_eq!(groups(sig(1, 64, 51), Level::Medium), 1);
        // 0.45: Low, not Medium.
        assert_eq!(groups(sig(1, 64, 102), Level::Medium), 0);
        assert_eq!(groups(sig(1, 64, 102), Level::Low), 1);
        // 0.30, and another scene: in no group.
        assert_eq!(groups(sig(1, 64, 153), Level::Low), 0);
        assert_eq!(groups(sig(2, 0, 0), Level::Low), 0);
    }

    #[test]
    fn blank_pages_match_only_blank_pages() {
        let blank = |v: u8| Signature { p: 0, d: 0, c: vec![v; 48], h: sig(3, 0, 0).h };
        let text = Signature { p: 0, d: 0, ..sig(3, 0, 0) };
        assert_eq!(cluster_similar(&all(&[blank(250), blank(251)]), Level::High).len(), 1);
        // The same palette and hashes, but the layout of one has contrast.
        assert_eq!(cluster_similar(&all(&[blank(250), text]), Level::Low).len(), 0);
    }

    #[test]
    fn similar_photos_group_at_any_position() {
        let v = [sig(1, 0, 0), sig(2, 0, 0), sig(1, 3, 0), sig(2, 2, 0), sig(3, 0, 0), sig(1, 1, 0)];
        assert_eq!(cluster_similar(&all(&v), Level::Same), vec![vec![0, 2, 5], vec![1, 3]]);
        // A photo without a signature is in no group.
        let mut gaps = all(&v);
        gaps[2] = None;
        assert_eq!(cluster_similar(&gaps, Level::Same), vec![vec![0, 5], vec![1, 3]]);
    }

    #[test]
    fn a_dense_burst_stops_at_a_long_gap_or_a_different_photo() {
        let (a, b) = (sig(1, 0, 0), sig(2, 0, 0));
        let items = [
            (100, Some(&a)),
            (100, Some(&a)),
            (101, Some(&a)),
            // Similar, but 30 s later: not in the burst.
            (131, Some(&a)),
            (132, Some(&a)),
            // In time, but another photo.
            (133, Some(&b)),
            (134, Some(&b)),
            (200, Some(&a)),
        ];
        assert_eq!(cluster_bursts(&items, BurstKind::Dense), vec![vec![0, 1, 2], vec![3, 4], vec![5, 6]]);
        // Without the time limit the similar neighbors are 1 burst.
        assert_eq!(cluster_bursts(&items, BurstKind::Sparse), vec![vec![0, 1, 2, 3, 4], vec![5, 6]]);
    }

    #[test]
    fn a_slow_series_keeps_its_rhythm() {
        let a = sig(1, 0, 0);
        // Gaps of 3 s: the limit grows to 2.5 × 3 + 1 = 8 s, so a gap of 6 s still fits; 20 s does not.
        let items: Vec<(i64, Option<&Signature>)> = [0, 3, 6, 9, 15, 35].iter().map(|&t| (t, Some(&a))).collect();
        assert_eq!(cluster_bursts(&items, BurstKind::Dense), vec![vec![0, 1, 2, 3, 4]]);
    }

    #[test]
    fn the_order_puts_similar_photos_side_by_side() {
        let v = [sig(1, 0, 0), sig(2, 0, 0), sig(1, 2, 0), sig(2, 4, 0), sig(1, 6, 0)];
        let refs: Vec<&Signature> = v.iter().collect();
        assert_eq!(similarity_order(&refs), vec![0, 2, 4, 1, 3]);
        assert!(similarity_order(&[]).is_empty());
    }
}
