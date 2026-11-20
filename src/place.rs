//! Finding free room for labels: a label is written only where it touches neither a curve
//! nor another label. Used for the plots on screen (again at every zoom) and for saved files.

/// x0, y0, x1, y1 with y growing downwards.
pub type Area = [f64; 4];

pub struct Placer {
    frame: Area,
    taken: Vec<Area>,
    lines: Vec<Vec<(f64, f64)>>,
}

/// Does the segment a-b touch the area? (Liang-Barsky clipping)
fn segment_hits(a: (f64, f64), b: (f64, f64), r: &Area) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.0 - r[0]), (dx, r[2] - a.0), (-dy, a.1 - r[1]), (dy, r[3] - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

impl Placer {
    pub fn new(frame: Area) -> Self {
        Placer { frame, taken: vec![], lines: vec![] }
    }

    /// Add a curve; points that are not finite break it into pieces.
    pub fn add_line(&mut self, pts: impl IntoIterator<Item = (f64, f64)>) {
        let mut cur: Vec<(f64, f64)> = vec![];
        let mut last_skipped = None;
        for p in pts {
            if !p.0.is_finite() || !p.1.is_finite() {
                if let Some(q) = last_skipped.take() {
                    cur.push(q);
                }
                if cur.len() > 1 {
                    self.lines.push(std::mem::take(&mut cur));
                }
                cur.clear();
                continue;
            }
            match cur.last() {
                Some(l) if (p.0 - l.0).abs() < 1.5 && (p.1 - l.1).abs() < 1.5 => last_skipped = Some(p),
                _ => {
                    cur.push(p);
                    last_skipped = None;
                }
            }
        }
        if let Some(q) = last_skipped {
            cur.push(q);
        }
        if cur.len() > 1 {
            self.lines.push(cur);
        }
    }

    pub fn reserve(&mut self, r: Area) {
        self.taken.push(r);
    }

    pub fn is_free(&self, r: Area) -> bool {
        let f = &self.frame;
        if r[0] < f[0] || r[1] < f[1] || r[2] > f[2] || r[3] > f[3] {
            return false;
        }
        if self.taken.iter().any(|t| r[0] < t[2] && r[2] > t[0] && r[1] < t[3] && r[3] > t[1]) {
            return false;
        }
        self.lines.iter().all(|l| l.windows(2).all(|s| !segment_hits(s[0], s[1], &r)))
    }

    /// The first free spot among `centres` for a label of this size; the spot is then taken.
    pub fn place(&mut self, w: f64, h: f64, centres: &[(f64, f64)]) -> Option<(f64, f64)> {
        let pad = 2.0;
        for c in centres {
            let r = [c.0 - w / 2.0 - pad, c.1 - h / 2.0 - pad, c.0 + w / 2.0 + pad, c.1 + h / 2.0 + pad];
            if self.is_free(r) {
                self.taken.push(r);
                return Some(*c);
            }
        }
        None
    }

    /// Spots touching the top of a spike: above it, then on either side. Never anywhere else,
    /// because a name far from its spike would say nothing.
    pub fn beside_tip(&self, x: f64, y: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
        vec![(x, y - h / 2.0 - 4.0), (x + w / 2.0 + 6.0, y + h / 2.0 + 1.0), (x - w / 2.0 - 6.0, y + h / 2.0 + 1.0)]
    }

    /// Spots beside a vertical marker at x, from the bottom of the plot upwards.
    pub fn beside_marker(&self, x: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
        let mut v = vec![];
        let f = self.frame;
        let step = h + 4.0;
        let rows = (((f[3] - f[1]) / step) as usize).min(40);
        for k in 0..rows {
            let y = f[3] - h / 2.0 - 4.0 - k as f64 * step;
            v.push((x - w / 2.0 - 4.0, y));
            v.push((x + w / 2.0 + 4.0, y));
        }
        v
    }

    /// Spots inside the band a..b, from the top of the plot downwards.
    pub fn inside_band(&self, a: f64, b: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
        let mut v = vec![];
        let f = self.frame;
        let (a, b) = (a.max(f[0]), b.min(f[2]));
        if b - a < w + 6.0 {
            return v;
        }
        let step = h + 4.0;
        let rows = (((f[3] - f[1]) / step) as usize).min(40);
        for k in 0..rows {
            let y = f[1] + h / 2.0 + 5.0 + k as f64 * step;
            for t in [0.5, 0.3, 0.7, 0.15, 0.85] {
                let x = a + (b - a) * t;
                if x - w / 2.0 >= a && x + w / 2.0 <= b {
                    v.push((x, y));
                }
            }
        }
        v
    }

    /// Spots around a preferred position.
    pub fn around(&self, x: f64, y: f64, h: f64) -> Vec<(f64, f64)> {
        let mut v = vec![(x, y)];
        for k in 1..=6 {
            let d = k as f64 * (h + 4.0);
            v.extend([(x, y - d), (x, y + d), (x - 2.0 * d, y), (x + 2.0 * d, y), (x - 2.0 * d, y - d), (x + 2.0 * d, y - d), (x - 2.0 * d, y + d), (x + 2.0 * d, y + d)]);
        }
        v
    }
}
