//! A force directed placement for the map (PRD 9.10, decision of 2026-11-14): nodes repel each
//! other, the two ends of an edge attract, so that who is connected to whom shows as clusters
//! and the frame is spread over. Deterministic: a fixed seed, a fixed number of iterations, the
//! same input giving the same places. Computed once, then still: nothing here animates, and the
//! caller computes again only when the structure of the map changes (CLAUDE.md law 6).

use egui::Vec2;

/// Iterations of the simulation, always this many.
pub const ITERATIONS: usize = 300;
/// The seed of the small displacement that breaks the symmetry of the starting places.
pub const SEED: u64 = 0x0a5c_0de5_eed0_0001;
/// Passes at most that push apart two nodes still overlapping after the simulation.
pub const SEPARATION_PASSES: usize = 200;

/// What the simulation places.
pub struct Input<'a> {
    /// The size of each node, its footprint, gap included by the caller if it wants one.
    pub sizes: &'a [Vec2],
    /// The pairs of nodes an edge joins. A pair may appear more than once; a loop is ignored.
    pub edges: &'a [(usize, usize)],
    /// Where each node starts, inside the frame.
    pub start: &'a [Vec2],
    /// The centre a node is held at, when it has one: it does not move, the others arrange
    /// around it.
    pub pinned: &'a [Option<Vec2>],
    /// The frame, from the origin: the centres of the free nodes are kept inside it.
    pub frame: Vec2,
}

/// A small generator of the displacements, xorshift64*, so that the places depend on the seed
/// alone and not on any library's notion of randomness.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
        // In [-1, 1).
        (v >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }
}

/// Whether two nodes at `a` and `b`, of sizes `sa` and `sb`, overlap, and by how much on each
/// axis when they do.
fn overlap(a: Vec2, sa: Vec2, b: Vec2, sb: Vec2) -> Option<Vec2> {
    let reach = (sa + sb) / 2.0;
    let d = (a - b).abs();
    (d.x < reach.x && d.y < reach.y).then(|| reach - d)
}

/// The centres of the nodes, inside the frame where they are free.
pub fn place(input: &Input<'_>) -> Vec<Vec2> {
    let n = input.sizes.len();
    let mut pos: Vec<Vec2> = (0..n)
        .map(|i| input.pinned[i].unwrap_or(input.start[i]))
        .collect();
    if n == 0 {
        return pos;
    }
    let free = |i: usize| input.pinned[i].is_none();
    let mut rng = Rng(SEED);
    for (i, p) in pos.iter_mut().enumerate() {
        let jitter = Vec2::new(rng.next(), rng.next());
        if free(i) {
            *p += jitter;
        }
    }
    let frame = input.frame.max(Vec2::splat(1.0));
    // The ideal distance between two nodes: the frame shared out among them, and never less
    // than the largest node and a gap, so that a crowded frame grows apart rather than overlaps.
    let largest = input
        .sizes
        .iter()
        .fold(Vec2::ZERO, |a, s| a.max(*s))
        .length();
    let k = (frame.x * frame.y / n as f32).sqrt().max(largest * 0.75);
    let inside = |i: usize, p: Vec2| -> Vec2 {
        let half = (input.sizes[i] / 2.0).min(frame / 2.0);
        p.clamp(half, frame - half)
    };
    let centre = frame / 2.0;
    let start_temperature = frame.max_elem() / 10.0;
    let mut disp = vec![Vec2::ZERO; n];
    for step in 0..ITERATIONS {
        let temperature = start_temperature * (1.0 - step as f32 / ITERATIONS as f32);
        disp.iter_mut().for_each(|d| *d = Vec2::ZERO);
        for i in 0..n {
            for j in (i + 1)..n {
                let delta = pos[i] - pos[j];
                let d = delta.length().max(0.01);
                let dir = if delta.length() > 0.0 {
                    delta / d
                } else {
                    Vec2::new(1.0, 0.0)
                };
                let mut f = k * k / d;
                // Two boxes on each other push apart harder than two points.
                if overlap(pos[i], input.sizes[i], pos[j], input.sizes[j]).is_some() {
                    f += k;
                }
                disp[i] += dir * f;
                disp[j] -= dir * f;
            }
        }
        for &(a, b) in input.edges {
            if a == b || a >= n || b >= n {
                continue;
            }
            let delta = pos[a] - pos[b];
            let d = delta.length().max(0.01);
            let f = d * d / k;
            let dir = delta / d;
            disp[a] -= dir * f;
            disp[b] += dir * f;
        }
        for i in (0..n).filter(|i| free(*i)) {
            // A light pull to the centre, so that a node joined to nothing stays near.
            disp[i] += (centre - pos[i]) * 0.05;
            let len = disp[i].length();
            if len > 0.0 {
                pos[i] += disp[i] / len * len.min(temperature);
            }
            pos[i] = inside(i, pos[i]);
        }
    }
    // What the simulation left overlapping is pushed apart, the least way, free nodes only.
    for _ in 0..SEPARATION_PASSES {
        let mut moved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                let Some(o) = overlap(pos[i], input.sizes[i], pos[j], input.sizes[j]) else {
                    continue;
                };
                let (fi, fj) = (free(i), free(j));
                if !fi && !fj {
                    continue;
                }
                let delta = pos[i] - pos[j];
                let push = if o.x < o.y {
                    Vec2::new(if delta.x >= 0.0 { o.x } else { -o.x }, 0.0)
                } else {
                    Vec2::new(0.0, if delta.y >= 0.0 { o.y } else { -o.y })
                };
                // A hair beyond touching, so that the pair is apart once moved.
                let push = push * 1.001 + push.normalized() * 0.5;
                match (fi, fj) {
                    (true, true) => {
                        pos[i] += push / 2.0;
                        pos[j] -= push / 2.0;
                    }
                    (true, false) => pos[i] += push,
                    _ => pos[j] -= push,
                }
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    pos
}

/// Whether any two of the nodes at `pos`, of `sizes`, overlap.
pub fn any_overlap(pos: &[Vec2], sizes: &[Vec2]) -> bool {
    (0..pos.len()).any(|i| {
        ((i + 1)..pos.len()).any(|j| overlap(pos[i], sizes[i], pos[j], sizes[j]).is_some())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        sizes: &'a [Vec2],
        edges: &'a [(usize, usize)],
        start: &'a [Vec2],
        pinned: &'a [Option<Vec2>],
    ) -> Input<'a> {
        Input {
            sizes,
            edges,
            start,
            pinned,
            frame: Vec2::new(1000.0, 700.0),
        }
    }

    /// The same input gives the same places; no two nodes overlap; a pinned node stays where it
    /// is held; nodes joined by an edge end nearer each other than nodes that are not.
    #[test]
    fn places_are_repeatable_apart_and_clustered_by_edges() {
        let sizes = vec![Vec2::new(200.0, 80.0); 6];
        // Two clusters of three: 0, 1, 2 and 3, 4, 5.
        let edges = [(1, 0), (2, 0), (4, 3), (5, 3)];
        let start: Vec<Vec2> = (0..6)
            .map(|i| Vec2::new(500.0 + i as f32, 350.0 - i as f32))
            .collect();
        let mut pinned = vec![None; 6];
        pinned[3] = Some(Vec2::new(150.0, 100.0));
        let a = place(&input(&sizes, &edges, &start, &pinned));
        let b = place(&input(&sizes, &edges, &start, &pinned));
        assert_eq!(a, b, "the same places");
        assert!(!any_overlap(&a, &sizes), "{a:?}");
        assert_eq!(a[3], Vec2::new(150.0, 100.0), "held where pinned");
        let d = |i: usize, j: usize| (a[i] - a[j]).length();
        let within = (d(0, 1) + d(0, 2) + d(3, 4) + d(3, 5)) / 4.0;
        let across = (d(0, 3) + d(1, 4) + d(2, 5)) / 3.0;
        assert!(
            within < across,
            "joined {within:.0} against apart {across:.0}"
        );

        // A frame far too small: the simulation cannot keep them apart inside it, the passes
        // after it do, out of the frame if they must.
        let cramped = Input {
            sizes: &sizes,
            edges: &edges,
            start: &start,
            pinned: &[None; 6],
            frame: Vec2::new(120.0, 90.0),
        };
        let c = place(&cramped);
        assert!(!any_overlap(&c, &sizes), "apart in a cramped frame: {c:?}");
    }
}
