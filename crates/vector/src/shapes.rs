//! Parametric ("live") shapes → paths: rectangles with corner radii, ellipses, polygons, stars
//! and lines. Knots run clockwise on screen (y down), starting at the top.

use photocraft_doc::{Knot, LiveShape, Path, Subpath};
use photocraft_geom::Point;

/// Handle length of a quarter-circle cubic, `4/3·(√2 − 1)`.
pub const KAPPA: f64 = 0.552_284_749_830_793_4;

fn smooth(anchor: (f64, f64), inc: (f64, f64), out: (f64, f64)) -> Knot {
    Knot::smooth(Point::new(anchor.0, anchor.1), Point::new(inc.0, inc.1), Point::new(out.0, out.1))
}

/// Axis-aligned rectangle.
pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
    Path::new(vec![Subpath::polygon(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)])])
}

/// Clamps corner radii `[tl, tr, br, bl]` so adjacent radii never exceed a side (scaled down
/// uniformly, like CSS `border-radius`).
pub fn clamp_radii(w: f64, h: f64, radii: [f64; 4]) -> [f64; 4] {
    let r = radii.map(|v| v.max(0.0));
    let mut f: f64 = 1.0;
    for (a, b, side) in [(r[0], r[1], w), (r[1], r[2], h), (r[2], r[3], w), (r[3], r[0], h)] {
        if a + b > side && a + b > 0.0 {
            f = f.min(side / (a + b));
        }
    }
    r.map(|v| v * f)
}

/// Rectangle with corner radii `[top-left, top-right, bottom-right, bottom-left]`. A negative
/// width or height extends from the origin to the left or up, like [`ellipse`] and [`polygon`].
pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, radii: [f64; 4]) -> Path {
    let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
    let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
    let [tl, tr, br, bl] = clamp_radii(w, h, radii);
    if tl <= 0.0 && tr <= 0.0 && br <= 0.0 && bl <= 0.0 {
        return rect(x, y, w, h);
    }
    let (x1, y1) = (x + w, y + h);
    let mut k = Vec::with_capacity(8);
    let corner = |p: (f64, f64)| Knot::corner(p.0, p.1);
    let push_arc = |k: &mut Vec<Knot>, start: (f64, f64), end: (f64, f64), c_start: (f64, f64), c_end: (f64, f64), r: f64| {
        if r <= 0.0 {
            k.push(corner(start));
            return;
        }
        k.push(Knot { anchor: Point::new(start.0, start.1), in_ctrl: Point::new(start.0, start.1), out_ctrl: Point::new(c_start.0, c_start.1), smooth: false });
        k.push(Knot { anchor: Point::new(end.0, end.1), in_ctrl: Point::new(c_end.0, c_end.1), out_ctrl: Point::new(end.0, end.1), smooth: false });
    };
    // Each corner arc from the end of one edge to the start of the next (clockwise).
    push_arc(&mut k, (x1 - tr, y), (x1, y + tr), (x1 - tr + KAPPA * tr, y), (x1, y + tr - KAPPA * tr), tr);
    push_arc(&mut k, (x1, y1 - br), (x1 - br, y1), (x1, y1 - br + KAPPA * br), (x1 - br + KAPPA * br, y1), br);
    push_arc(&mut k, (x + bl, y1), (x, y1 - bl), (x + bl - KAPPA * bl, y1), (x, y1 - bl + KAPPA * bl), bl);
    push_arc(&mut k, (x, y + tl), (x + tl, y), (x, y + tl - KAPPA * tl), (x + tl - KAPPA * tl, y), tl);
    // Start at the top-left arc end so the first knot is on the top edge.
    let n = k.len();
    k.rotate_left(n - 1);
    // Drop duplicated consecutive anchors (zero-length straight edges).
    let mut out: Vec<Knot> = Vec::with_capacity(k.len());
    for kn in k {
        if let Some(last) = out.last_mut()
            && (last.anchor.x - kn.anchor.x).abs() < 1e-12
            && (last.anchor.y - kn.anchor.y).abs() < 1e-12
        {
            last.out_ctrl = kn.out_ctrl;
            continue;
        }
        out.push(kn);
    }
    if out.len() > 1 {
        let (f, l) = (out[0], out[out.len() - 1]);
        if (f.anchor.x - l.anchor.x).abs() < 1e-12 && (f.anchor.y - l.anchor.y).abs() < 1e-12 {
            out[0].in_ctrl = l.in_ctrl;
            out.pop();
        }
    }
    Path::new(vec![Subpath { closed: true, knots: out, op: Default::default() }])
}

/// Ellipse inscribed in the rectangle.
pub fn ellipse(x: f64, y: f64, w: f64, h: f64) -> Path {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (cx, cy) = (x + rx, y + ry);
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let knots = vec![
        smooth((cx, cy - ry), (cx - kx, cy - ry), (cx + kx, cy - ry)),
        smooth((cx + rx, cy), (cx + rx, cy - ky), (cx + rx, cy + ky)),
        smooth((cx, cy + ry), (cx + kx, cy + ry), (cx - kx, cy + ry)),
        smooth((cx - rx, cy), (cx - rx, cy + ky), (cx - rx, cy - ky)),
    ];
    Path::new(vec![Subpath { closed: true, knots, op: Default::default() }])
}

/// Regular polygon (or star when `star_ratio < 1`) inscribed in the rectangle's ellipse, first
/// vertex at the top.
pub fn polygon(x: f64, y: f64, w: f64, h: f64, sides: u32, star_ratio: f64) -> Path {
    let sides = sides.clamp(3, 100) as usize;
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (cx, cy) = (x + rx, y + ry);
    let star = star_ratio > 0.0 && star_ratio < 1.0;
    let count = if star { sides * 2 } else { sides };
    let pts: Vec<(f64, f64)> = (0..count)
        .map(|i| {
            let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * i as f64 / count as f64;
            let k = if star && i % 2 == 1 { star_ratio } else { 1.0 };
            (cx + rx * k * a.cos(), cy + ry * k * a.sin())
        })
        .collect();
    Path::new(vec![Subpath::polygon(&pts)])
}

/// A line drawn as a filled bar of `weight` px from `a` to `b`.
pub fn line(a: (f64, f64), b: (f64, f64), weight: f64) -> Path {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = dx.hypot(dy);
    let hw = weight.max(0.0) / 2.0;
    let (nx, ny) = if l > 0.0 { (-dy / l * hw, dx / l * hw) } else { (0.0, hw) };
    Path::new(vec![Subpath::polygon(&[(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)])])
}

/// The path of a live shape.
pub fn live_path(s: &LiveShape) -> Path {
    match s {
        LiveShape::Rect { rect: [x, y, w, h], radii } => rounded_rect(*x, *y, *w, *h, *radii),
        LiveShape::Ellipse { rect: [x, y, w, h] } => ellipse(*x, *y, *w, *h),
        LiveShape::Polygon { rect: [x, y, w, h], sides, star_ratio } => polygon(*x, *y, *w, *h, *sides, *star_ratio),
        LiveShape::Line { from, to, weight } => line((from[0], from[1]), (to[0], to[1]), *weight),
        LiveShape::Rounded { base, radii } => round_corners(base, radii),
    }
}

fn sharp(k: &Knot) -> bool {
    let same = |a: Point, b: Point| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
    same(k.in_ctrl, k.anchor) && same(k.out_ctrl, k.anchor)
}

/// The corners of `path` that can be rounded: sharp knots with a segment on both sides.
fn rounds(sp: &Subpath, i: usize) -> bool {
    let n = sp.knots.len();
    n >= 3 && sharp(&sp.knots[i]) && (sp.closed || (i > 0 && i + 1 < n))
}

/// How many corners of `path` can be rounded.
pub fn corner_count(path: &Path) -> usize {
    path.subpaths.iter().map(|sp| (0..sp.knots.len()).filter(|&i| rounds(sp, i)).count()).sum()
}

/// `path` with each of its sharp corners rounded by the circular arc of the radius in `radii`
/// (one number: the same for every corner; else one per corner in path order, 0 = left sharp).
/// A radius is cut down to what the two edges at the corner leave: half of the shorter one.
pub fn round_corners(path: &Path, radii: &[f64]) -> Path {
    let mut out = path.clone();
    let mut index = 0usize;
    for (sp, dst) in path.subpaths.iter().zip(out.subpaths.iter_mut()) {
        let n = sp.knots.len();
        let mut knots = Vec::with_capacity(n * 2);
        for i in 0..n {
            let k = sp.knots[i];
            if !rounds(sp, i) {
                knots.push(k);
                continue;
            }
            let r = if radii.len() == 1 { radii[0] } else { radii.get(index).copied().unwrap_or(0.0) };
            index += 1;
            let (prev, next) = (&sp.knots[(i + n - 1) % n], &sp.knots[(i + 1) % n]);
            let p = k.anchor;
            // The tangent of each neighbouring segment where it leaves the corner.
            let toward = |from: &Knot, ctrl: Point| {
                let c = if (ctrl.x - from.anchor.x).abs() > 1e-9 || (ctrl.y - from.anchor.y).abs() > 1e-9 { ctrl } else { from.anchor };
                (c.x - p.x, c.y - p.y)
            };
            let (a, b) = (toward(prev, prev.out_ctrl), toward(next, next.in_ctrl));
            let (la, lb) = (a.0.hypot(a.1), b.0.hypot(b.1));
            let (da, db) = ((prev.anchor.x - p.x).hypot(prev.anchor.y - p.y), (next.anchor.x - p.x).hypot(next.anchor.y - p.y));
            if r <= 0.0 || la < 1e-9 || lb < 1e-9 {
                knots.push(k);
                continue;
            }
            let (ua, ub) = ((a.0 / la, a.1 / la), (b.0 / lb, b.1 / lb));
            let theta = (ua.0 * ub.0 + ua.1 * ub.1).clamp(-1.0, 1.0).acos();
            // Straight on or folded back on itself: nothing to round.
            if !(0.01..std::f64::consts::PI - 0.01).contains(&theta) {
                knots.push(k);
                continue;
            }
            let half = (theta / 2.0).tan();
            let t = (r / half).min(da.min(db) / 2.0);
            let r_eff = t * half;
            let h = 4.0 / 3.0 * ((std::f64::consts::PI - theta) / 4.0).tan() * r_eff;
            let pa = Point::new(p.x + ua.0 * t, p.y + ua.1 * t);
            let pb = Point::new(p.x + ub.0 * t, p.y + ub.1 * t);
            knots.push(Knot { anchor: pa, in_ctrl: pa, out_ctrl: Point::new(pa.x - ua.0 * h, pa.y - ua.1 * h), smooth: false });
            knots.push(Knot { anchor: pb, in_ctrl: Point::new(pb.x - ub.0 * h, pb.y - ub.1 * h), out_ctrl: pb, smooth: false });
        }
        dst.knots = knots;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_rect_knots() {
        let p = rounded_rect(0.0, 0.0, 100.0, 50.0, [10.0; 4]);
        assert_eq!(p.subpaths[0].knots.len(), 8);
        let sharp = rounded_rect(0.0, 0.0, 100.0, 50.0, [0.0; 4]);
        assert_eq!(sharp.subpaths[0].knots.len(), 4);
        let mixed = rounded_rect(0.0, 0.0, 100.0, 50.0, [10.0, 0.0, 10.0, 0.0]);
        assert_eq!(mixed.subpaths[0].knots.len(), 6);
        assert_eq!(clamp_radii(10.0, 10.0, [10.0; 4]), [5.0; 4]);
    }

    #[test]
    fn negative_size_rect_extends_from_its_origin_like_the_ellipse() {
        // Issue #4203: a negative width/height drew the rectangle on the far side of its origin.
        let want = ellipse(50.0, 10.0, -20.0, 30.0).control_bounds().unwrap();
        let want_neg_h = ellipse(50.0, 40.0, -20.0, -30.0).control_bounds().unwrap();
        for radii in [[0.0; 4], [5.0; 4]] {
            let b = rounded_rect(50.0, 10.0, -20.0, 30.0, radii).control_bounds().unwrap();
            assert_eq!(b, want, "radii {radii:?}");
            assert_eq!(b, (30.0, 10.0, 50.0, 40.0), "radii {radii:?}");
            let b = rounded_rect(50.0, 40.0, -20.0, -30.0, radii).control_bounds().unwrap();
            assert_eq!(b, want_neg_h, "radii {radii:?}");
        }
    }

    #[test]
    fn corners_round_one_by_one_or_all() {
        let tri = polygon(0.0, 0.0, 100.0, 100.0, 3, 1.0);
        assert_eq!(corner_count(&tri), 3);
        assert_eq!(round_corners(&tri, &[10.0]).subpaths[0].knots.len(), 6);
        assert_eq!(round_corners(&tri, &[10.0, 0.0, 0.0]).subpaths[0].knots.len(), 4);
        assert_eq!(round_corners(&tri, &[0.0]).subpaths[0].knots.len(), 3);
        // A rounded square's arc starts on the edge, a radius away from the corner.
        let sq = rect(0.0, 0.0, 100.0, 100.0);
        let r = round_corners(&sq, &[20.0]);
        let first = r.subpaths[0].knots[0].anchor;
        assert!((first.x - 0.0).abs() < 1e-9 && (first.y - 20.0).abs() < 1e-9, "{first:?}");
    }

    #[test]
    fn star_vertices() {
        assert_eq!(polygon(0.0, 0.0, 10.0, 10.0, 5, 0.5).subpaths[0].knots.len(), 10);
        assert_eq!(polygon(0.0, 0.0, 10.0, 10.0, 6, 1.0).subpaths[0].knots.len(), 6);
        let top = polygon(0.0, 0.0, 10.0, 10.0, 3, 1.0).subpaths[0].knots[0].anchor;
        assert!((top.x - 5.0).abs() < 1e-9 && top.y.abs() < 1e-9);
    }
}
