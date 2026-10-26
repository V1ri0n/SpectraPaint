//! Evaluates a colour-grading node graph ([`photocraft_doc::grade`]) over a buffer.
//!
//! The graph is first prepared (curve tables built, nodes put in reading order), then every pixel
//! runs through it: each node gets the pixel its inputs produced and gives one back. A node with a
//! matte (a qualifier, windows, a gain) grades the whole pixel and then mixes the result back by
//! its key, so the matte decides how much of the grade shows.
//!
//! All the maths works on the document's own (encoded) channel values and keeps values outside
//! 0..=1 until a step needs them in range (hue and the qualifier work in HSL), so a high-dynamic
//! range document keeps its highlights through the wheels. The formulas are this program's own.

use photocraft_doc::grade::{Corrector, GradeWindow, LutNode, Matte, MixLayer, NodeGraph, NodeKind, Qualifier, WindowShape};
use photocraft_geom::Rect;
use rayon::prelude::*;

use crate::Buffer;
use crate::adjust::{curve_lut, hsl_to_rgb, lut3d_sample, rgb_to_hsl};

/// Rec. 709 luma weights, on the encoded values.
fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 1 inside `lo..=hi`, fading to 0 over `soft` outside either end.
fn range_key(v: f32, lo: f32, hi: f32, soft: f32) -> f32 {
    if v >= lo && v <= hi {
        return 1.0;
    }
    if soft <= 1e-6 {
        return 0.0;
    }
    let outside = if v < lo { lo - v } else { v - hi };
    smooth(1.0 - outside / soft)
}

// ---------------------------------------------------------------------------------------------
// Curves

/// Entries in a prepared hue-axis or tone-axis curve.
const TABLE: usize = 256;

/// A list of `[x, change]` points as a table over x in 0..=1: the points joined by smooth steps
/// (flat at each point), constant beyond the ends, or wrapping round when `periodic`.
fn curve_table(points: &[[f32; 2]], periodic: bool) -> Option<Vec<f32>> {
    let mut pts: Vec<[f32; 2]> = points.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| [p[0].clamp(0.0, 1.0), p[1].clamp(-1.0, 1.0)]).collect();
    if pts.is_empty() || pts.iter().all(|p| p[1] == 0.0) {
        return None;
    }
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let n = pts.len();
    Some(
        (0..TABLE)
            .map(|k| {
                let x = k as f32 / (TABLE - 1) as f32;
                if n == 1 {
                    return pts[0][1];
                }
                if periodic {
                    // The segment holding x; the last point joins the first one a turn on.
                    let (a, b) = match pts.iter().position(|p| p[0] > x) {
                        Some(0) => ([pts[n - 1][0] - 1.0, pts[n - 1][1]], pts[0]),
                        None => (pts[n - 1], [pts[0][0] + 1.0, pts[0][1]]),
                        Some(i) => (pts[i - 1], pts[i]),
                    };
                    a[1] + (b[1] - a[1]) * smooth((x - a[0]) / (b[0] - a[0]).max(1e-6))
                } else if x <= pts[0][0] {
                    pts[0][1]
                } else if x >= pts[n - 1][0] {
                    pts[n - 1][1]
                } else {
                    let i = pts.iter().position(|p| p[0] > x).unwrap_or(n - 1);
                    let (a, b) = (pts[i - 1], pts[i]);
                    a[1] + (b[1] - a[1]) * smooth((x - a[0]) / (b[0] - a[0]).max(1e-6))
                }
            })
            .collect(),
    )
}

fn table_at(t: &[f32], x: f32, periodic: bool) -> f32 {
    let x = if periodic { x.rem_euclid(1.0) } else { x.clamp(0.0, 1.0) };
    let p = x * (TABLE - 1) as f32;
    let i = p.floor() as usize;
    let j = if periodic { (i + 1) % TABLE } else { (i + 1).min(TABLE - 1) };
    let f = p - i as f32;
    let a = t.get(i).copied().unwrap_or(0.0);
    let b = t.get(j).copied().unwrap_or(a);
    a + (b - a) * f
}

fn lookup(table: &[f32], v: f32) -> f32 {
    let last = table.len().saturating_sub(1);
    if last == 0 {
        return v;
    }
    let p = v.clamp(0.0, 1.0) * last as f32;
    let i = p.floor() as usize;
    let j = (i + 1).min(last);
    let f = p - i as f32;
    table[i] + (table[j] - table[i]) * f
}

// ---------------------------------------------------------------------------------------------
// The matte

struct PMatte {
    q: Option<Qualifier>,
    windows: Vec<GradeWindow>,
    gain: f32,
}

impl PMatte {
    fn new(m: &Matte, ignore_windows: bool) -> PMatte {
        PMatte {
            q: (!m.qualifier.is_open()).then(|| m.qualifier.clone()),
            windows: if ignore_windows { Vec::new() } else { m.windows.iter().filter(|w| w.enabled).cloned().collect() },
            gain: m.gain.clamp(0.0, 1.0),
        }
    }

    fn is_open(&self) -> bool {
        self.q.is_none() && self.windows.is_empty() && self.gain >= 1.0
    }

    /// How much of the grade shows at this pixel: `c` is the node's input colour, `(px, py)` the
    /// pixel position in pixels from the canvas corner, `(w, h)` the canvas size.
    fn key(&self, c: [f32; 3], px: f32, py: f32, w: f32, h: f32) -> f32 {
        let mut k = self.gain;
        if let Some(q) = &self.q {
            let (hue, sat, lum) = rgb_to_hsl(c.map(|v| v.clamp(0.0, 1.0)));
            let mut m = 1.0;
            if q.use_hue {
                let d = (hue - q.hue_center).rem_euclid(1.0);
                let d = d.min(1.0 - d);
                m *= if d <= q.hue_width {
                    1.0
                } else if q.hue_soft > 1e-6 {
                    smooth(1.0 - (d - q.hue_width) / q.hue_soft)
                } else {
                    0.0
                };
            }
            if q.use_sat {
                m *= range_key(sat, q.sat[0], q.sat[1], q.sat_soft);
            }
            if q.use_lum {
                m *= range_key(lum, q.lum[0], q.lum[1], q.lum_soft);
            }
            k *= if q.invert { 1.0 - m } else { m };
        }
        if !self.windows.is_empty() {
            let u = self.windows.iter().map(|win| window_key(win, px, py, w, h)).fold(0.0f32, f32::max);
            k *= u;
        }
        k
    }
}

/// A window's key at a pixel.
fn window_key(win: &GradeWindow, px: f32, py: f32, w: f32, h: f32) -> f32 {
    let (dx, dy) = (px - win.center[0] * w, py - win.center[1] * h);
    let a = win.rotation.to_radians();
    let (s, c) = a.sin_cos();
    // Into the window's own axes.
    let (lx, ly) = (dx * c + dy * s, -dx * s + dy * c);
    let (hw, hh) = ((win.size[0] * w / 2.0).max(1e-3), (win.size[1] * h / 2.0).max(1e-3));
    let soft = win.softness.clamp(0.0, 1.0);
    let edge = |d: f32| {
        // d is 1 at the window's edge; the fade is the inner `soft` part.
        if d <= 1.0 - soft {
            1.0
        } else if d >= 1.0 {
            0.0
        } else {
            1.0 - smooth((d - (1.0 - soft)) / soft.max(1e-6))
        }
    };
    let m = match win.shape {
        WindowShape::Circle => edge(((lx / hw).powi(2) + (ly / hh).powi(2)).sqrt()),
        WindowShape::Linear => edge((lx.abs() / hw).max(ly.abs() / hh)),
        // Full on the left, nothing on the right, over the window's width.
        WindowShape::Gradient => smooth(0.5 - lx / (2.0 * hw)),
    };
    if win.invert { 1.0 - m } else { m }
}

// ---------------------------------------------------------------------------------------------
// The corrector

struct PCorrector {
    lift: [f32; 3],
    gain: [f32; 3],
    exponent: [f32; 3],
    offset: [f32; 3],
    contrast: f32,
    pivot: f32,
    saturation: f32,
    hue_turn: f32,
    balance: Option<[f32; 3]>,
    boost: f32,
    shadows: f32,
    highlights: f32,
    y: Option<Vec<f32>>,
    ch: [Option<Vec<f32>>; 3],
    hue_vs_hue: Option<Vec<f32>>,
    hue_vs_sat: Option<Vec<f32>>,
    hue_vs_lum: Option<Vec<f32>>,
    lum_vs_sat: Option<Vec<f32>>,
    sat_vs_sat: Option<Vec<f32>>,
    matte: PMatte,
    neutral: bool,
}

fn prepared_curve(points: &[photocraft_doc::adjust::CurvePoint]) -> Option<Vec<f32>> {
    (!points.is_empty() && !photocraft_doc::adjust::is_identity_curve(points)).then(|| curve_lut(points))
}

impl PCorrector {
    fn new(c: &Corrector, ignore_windows: bool) -> PCorrector {
        let g = |w: &photocraft_doc::grade::Wheel| w.channels().map(|v| if v.is_finite() { v.clamp(-2.0, 2.0) } else { 0.0 });
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let (t, ti) = (finite(c.temperature, 0.0).clamp(-1.0, 1.0), finite(c.tint, 0.0).clamp(-1.0, 1.0));
        // Warm turns red up and blue down; magenta takes green down.
        let balance = (t != 0.0 || ti != 0.0).then(|| [1.0 + 0.25 * t, 1.0 - 0.25 * ti, 1.0 - 0.25 * t]);
        PCorrector {
            lift: g(&c.lift),
            gain: g(&c.gain).map(|v| 1.0 + v),
            // Gamma +1 is an exponent of 1/2: it lifts the mid-tones by a stop-ish.
            exponent: g(&c.gamma).map(|v| 2f32.powf(-v)),
            offset: g(&c.offset).map(|v| v * 0.5),
            contrast: finite(c.contrast, 1.0).clamp(0.0, 4.0),
            pivot: finite(c.pivot, 0.435).clamp(0.0, 1.0),
            saturation: finite(c.saturation, 1.0).clamp(0.0, 4.0),
            hue_turn: finite(c.hue, 0.0) / 360.0,
            balance,
            boost: finite(c.boost, 0.0).clamp(-1.0, 1.0),
            shadows: finite(c.shadows, 0.0).clamp(-1.0, 1.0),
            highlights: finite(c.highlights, 0.0).clamp(-1.0, 1.0),
            y: prepared_curve(&c.curves.y),
            ch: [prepared_curve(&c.curves.r), prepared_curve(&c.curves.g), prepared_curve(&c.curves.b)],
            hue_vs_hue: curve_table(&c.hue_curves.hue_vs_hue, true),
            hue_vs_sat: curve_table(&c.hue_curves.hue_vs_sat, true),
            hue_vs_lum: curve_table(&c.hue_curves.hue_vs_lum, true),
            lum_vs_sat: curve_table(&c.hue_curves.lum_vs_sat, false),
            sat_vs_sat: curve_table(&c.hue_curves.sat_vs_sat, false),
            matte: PMatte::new(&c.matte, ignore_windows),
            neutral: c.is_neutral(),
        }
    }

    fn needs_hsl(&self) -> bool {
        self.hue_turn != 0.0
            || self.hue_vs_hue.is_some()
            || self.hue_vs_sat.is_some()
            || self.hue_vs_lum.is_some()
            || self.lum_vs_sat.is_some()
            || self.sat_vs_sat.is_some()
    }

    /// The grade of one pixel, before the matte.
    fn correct(&self, input: [f32; 3]) -> [f32; 3] {
        if self.neutral {
            return input;
        }
        let mut v = input;
        if let Some(b) = self.balance {
            for i in 0..3 {
                v[i] *= b[i];
            }
        }
        for i in 0..3 {
            let mut x = v[i];
            x += self.lift[i] * (1.0 - x);
            x *= self.gain[i];
            if self.exponent[i] != 1.0 {
                x = x.max(0.0).powf(self.exponent[i]);
            }
            v[i] = x + self.offset[i];
        }
        if self.contrast != 1.0 {
            for x in &mut v {
                *x = (*x - self.pivot) * self.contrast + self.pivot;
            }
        }
        if self.shadows != 0.0 || self.highlights != 0.0 {
            let l = luma(v).clamp(0.0, 1.0);
            let add = 0.25 * (self.shadows * (1.0 - l) * (1.0 - l) + self.highlights * l * l);
            for x in &mut v {
                *x += add;
            }
        }
        if let Some(t) = &self.y {
            for x in &mut v {
                *x = lookup(t, *x);
            }
        }
        for i in 0..3 {
            if let Some(t) = &self.ch[i] {
                v[i] = lookup(t, v[i]);
            }
        }
        if self.boost != 0.0 {
            let (mx, mn) = (v[0].max(v[1]).max(v[2]), v[0].min(v[1]).min(v[2]));
            let s = if mx > 1e-6 { ((mx - mn) / mx).clamp(0.0, 1.0) } else { 0.0 };
            let k = 1.0 + self.boost * (1.0 - s);
            let l = luma(v);
            v = [l + (v[0] - l) * k, l + (v[1] - l) * k, l + (v[2] - l) * k];
        }
        if self.saturation != 1.0 {
            let l = luma(v);
            v = [l + (v[0] - l) * self.saturation, l + (v[1] - l) * self.saturation, l + (v[2] - l) * self.saturation];
        }
        if self.needs_hsl() {
            let (mut h, mut s, mut l) = rgb_to_hsl(v.map(|x| x.clamp(0.0, 1.0)));
            h += self.hue_turn;
            if let Some(t) = &self.hue_vs_hue {
                h += table_at(t, h, true) * 0.5;
            }
            if let Some(t) = &self.hue_vs_sat {
                s *= 1.0 + table_at(t, h, true);
            }
            if let Some(t) = &self.hue_vs_lum {
                l += table_at(t, h, true) * 0.5;
            }
            if let Some(t) = &self.lum_vs_sat {
                s *= 1.0 + table_at(t, l, false);
            }
            if let Some(t) = &self.sat_vs_sat {
                s *= 1.0 + table_at(t, s, false);
            }
            v = hsl_to_rgb(h.rem_euclid(1.0), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
        }
        v
    }
}

// ---------------------------------------------------------------------------------------------
// The graph

#[derive(Clone, Copy)]
enum Src {
    Source,
    Node(usize),
}

enum PKind {
    Corrector(Box<PCorrector>),
    Parallel { base: Src },
    Layers(Vec<MixLayer>),
    Lut { lut: Option<std::sync::Arc<Vec<f32>>>, size: usize, intensity: f32, tetrahedral: bool, matte: PMatte },
}

struct PNode {
    kind: PKind,
    inputs: Vec<Src>,
    bypass: bool,
}

/// A prepared graph, in reading order, ready to run on pixels.
pub struct Prepared {
    nodes: Vec<PNode>,
    /// The index of the output node in `nodes`.
    output: usize,
    neutral: bool,
}

#[derive(Clone, Copy, Default)]
struct Out {
    /// What the node passes on: the grade mixed in by its key.
    rgb: [f32; 3],
    /// The grade itself, whole.
    layer: [f32; 3],
    key: f32,
}

/// Prepares `graph` (an invalid graph gives None: it changes nothing). With `ignore_windows` the
/// windows are left out, as a LUT bake needs.
pub fn prepare(graph: &NodeGraph, ignore_windows: bool) -> Option<Prepared> {
    graph.validate().ok()?;
    let order = graph.order().ok()?;
    let index = |id: u32| order.iter().position(|o| *o == id);
    let src_of = |i: Option<u32>| -> Option<Src> {
        match i {
            None => Some(Src::Source),
            Some(id) => index(id).map(Src::Node),
        }
    };
    let mut nodes = Vec::with_capacity(order.len());
    for id in &order {
        let n = graph.node(*id)?;
        let inputs: Vec<Src> = n.inputs.iter().map(|i| src_of(*i)).collect::<Option<_>>()?;
        let kind = match &n.kind {
            NodeKind::Corrector(c) => PKind::Corrector(Box::new(PCorrector::new(c, ignore_windows))),
            NodeKind::ParallelMixer { base } => PKind::Parallel { base: src_of(*base)? },
            NodeKind::LayerMixer { layers } => PKind::Layers(layers.clone()),
            NodeKind::Lut(l) => {
                let LutNode { lut, size, intensity, tetrahedral, matte, .. } = l.as_ref();
                PKind::Lut { lut: lut.clone(), size: *size as usize, intensity: intensity.clamp(0.0, 1.0), tetrahedral: *tetrahedral, matte: PMatte::new(matte, ignore_windows) }
            }
        };
        nodes.push(PNode { kind, inputs, bypass: n.bypass });
    }
    let output = index(graph.output)?;
    let neutral = nodes.iter().all(|n| {
        n.bypass
            || match &n.kind {
                PKind::Corrector(c) => c.neutral,
                PKind::Parallel { .. } | PKind::Layers(_) => true,
                PKind::Lut { lut, .. } => lut.is_none(),
            }
    });
    Some(Prepared { nodes, output, neutral })
}

impl Prepared {
    /// Whether the graph leaves every pixel as it is.
    pub fn is_neutral(&self) -> bool {
        self.neutral
    }

    /// Runs one pixel (`c`, at `(px, py)` pixels from the canvas corner of a `w`×`h` canvas)
    /// through the graph. `scratch` holds the nodes' results and is reused between pixels.
    fn run(&self, c: [f32; 3], px: f32, py: f32, w: f32, h: f32, scratch: &mut Vec<Out>) -> [f32; 3] {
        scratch.clear();
        let source = Out { rgb: c, layer: c, key: 1.0 };
        for n in &self.nodes {
            let get = |s: Src, scratch: &Vec<Out>| match s {
                Src::Source => source,
                Src::Node(j) => scratch.get(j).copied().unwrap_or(source),
            };
            let first = n.inputs.first().map_or(source, |s| get(*s, scratch));
            let out = if n.bypass {
                Out { rgb: first.rgb, layer: first.rgb, key: 1.0 }
            } else {
                match &n.kind {
                    PKind::Corrector(pc) => {
                        let graded = pc.correct(first.rgb);
                        let key = if pc.matte.is_open() { 1.0 } else { pc.matte.key(first.rgb, px, py, w, h) };
                        Out { rgb: if key >= 1.0 { graded } else { lerp3(first.rgb, graded, key) }, layer: graded, key }
                    }
                    PKind::Lut { lut, size, intensity, tetrahedral, matte } => {
                        let graded = match lut {
                            Some(t) => lerp3(first.rgb, lut3d_sample(t, *size, first.rgb, *tetrahedral), *intensity),
                            None => first.rgb,
                        };
                        let key = if matte.is_open() { 1.0 } else { matte.key(first.rgb, px, py, w, h) };
                        Out { rgb: lerp3(first.rgb, graded, key), layer: graded, key }
                    }
                    PKind::Parallel { base } => {
                        let b = get(*base, scratch).rgb;
                        let mut sum = b;
                        for s in &n.inputs {
                            let o = get(*s, scratch).rgb;
                            for i in 0..3 {
                                sum[i] += o[i] - b[i];
                            }
                        }
                        Out { rgb: sum, layer: sum, key: 1.0 }
                    }
                    PKind::Layers(layers) => {
                        let mut acc = first.rgb;
                        for (k, s) in n.inputs.iter().enumerate().skip(1) {
                            let top = get(*s, scratch);
                            let m = layers.get(k).copied().unwrap_or_default();
                            let blended = crate::psblend::blend_rgb(m.mode, acc, top.layer);
                            acc = lerp3(acc, blended, (top.key * m.opacity).clamp(0.0, 1.0));
                        }
                        Out { rgb: acc, layer: acc, key: 1.0 }
                    }
                }
            };
            scratch.push(out);
        }
        scratch.get(self.output).map_or(c, |o| o.rgb)
    }
}

/// Applies `graph` to `buf`; `canvas` is the document canvas the windows are laid out on (the
/// buffer may be a band of it). Pixels with no alpha are left alone.
pub fn apply(graph: &NodeGraph, buf: &mut Buffer, canvas: Rect) {
    let Some(p) = prepare(graph, false) else { return };
    if p.is_neutral() {
        return;
    }
    let (w, h) = (canvas.width().max(1) as f32, canvas.height().max(1) as f32);
    let row = buf.rect.width() as usize;
    if row == 0 {
        return;
    }
    let (x0, y0) = (buf.rect.x0 - canvas.x0, buf.rect.y0 - canvas.y0);
    buf.px.par_chunks_mut(row).enumerate().for_each(|(ry, line)| {
        let mut scratch = Vec::with_capacity(p.nodes.len());
        let py = (y0 + ry as i32) as f32 + 0.5;
        for (rx, px) in line.iter_mut().enumerate() {
            if px[3] <= 0.0 {
                continue;
            }
            let o = p.run([px[0], px[1], px[2]], (x0 + rx as i32) as f32 + 0.5, py, w, h, &mut scratch);
            if o.iter().all(|v| v.is_finite()) {
                px[..3].copy_from_slice(&o);
            }
        }
    });
}

/// The graph as a 3D LUT of `size`³ entries (red varying fastest, in the `ColorLookup` layout),
/// 0..=1 in and out. Windows cannot be baked and are left out, and values are clamped to 0..=1.
pub fn bake_lut(graph: &NodeGraph, size: usize) -> Option<Vec<f32>> {
    let size = size.clamp(2, photocraft_doc::grade::MAX_LUT_SIZE as usize);
    let p = prepare(graph, true)?;
    let m = (size - 1) as f32;
    let mut out = Vec::with_capacity(size * size * size * 3);
    let mut scratch = Vec::new();
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let c = [r as f32 / m, g as f32 / m, b as f32 / m];
                let o = p.run(c, 0.0, 0.0, 1.0, 1.0, &mut scratch);
                out.extend(o.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }));
            }
        }
    }
    Some(out)
}

/// Whether any node of `graph` has an enabled window (what a LUT bake cannot keep).
pub fn has_windows(graph: &NodeGraph) -> bool {
    graph.nodes.iter().any(|n| n.kind.matte().is_some_and(|m| m.windows.iter().any(|w| w.enabled)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_color::BlendMode;
    use photocraft_doc::adjust::CurvePoint;
    use photocraft_doc::grade::Wheel;

    fn buf(w: i32, h: i32, f: impl Fn(i32, i32) -> [f32; 4]) -> Buffer {
        let rect = Rect::new(0, 0, w, h);
        let mut b = Buffer::transparent(rect);
        for y in 0..h {
            for x in 0..w {
                b.px[(y * w + x) as usize] = f(x, y);
            }
        }
        b
    }

    fn corrector(g: &mut NodeGraph, id: u32) -> &mut Corrector {
        match g.node_mut(id).map(|n| &mut n.kind) {
            Some(NodeKind::Corrector(c)) => c,
            _ => panic!("not a corrector"),
        }
    }

    fn one(g: &NodeGraph, c: [f32; 3]) -> [f32; 3] {
        let mut b = buf(1, 1, |_, _| [c[0], c[1], c[2], 1.0]);
        apply(g, &mut b, Rect::new(0, 0, 1, 1));
        [b.px[0][0], b.px[0][1], b.px[0][2]]
    }

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    #[test]
    fn a_neutral_graph_changes_nothing() {
        let g = NodeGraph::default();
        assert!(prepare(&g, false).is_some_and(|p| p.is_neutral()));
        for c in [[0.1, 0.5, 0.9], [1.2, -0.1, 0.3], [0.0; 3]] {
            assert_eq!(one(&g, c), c);
        }
        // An invalid graph changes nothing either.
        let mut bad = g.clone();
        bad.output = 9;
        assert_eq!(one(&bad, [0.3; 3]), [0.3; 3]);
    }

    #[test]
    fn lift_gamma_gain_and_offset_do_what_their_names_say() {
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).gain = Wheel { rgb: [0.0; 3], master: 1.0 };
        assert!(near(one(&g, [0.25, 0.5, 0.1]), [0.5, 1.0, 0.2]), "gain doubles");
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).lift = Wheel { rgb: [0.0; 3], master: 0.1 };
        assert!(near(one(&g, [0.0, 1.0, 0.5]), [0.1, 1.0, 0.55]), "lift raises the black point, white stays");
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).gamma = Wheel { rgb: [0.0; 3], master: 1.0 };
        assert!(near(one(&g, [0.25, 0.0, 1.0]), [0.5, 0.0, 1.0]), "gamma bends the middle, not the ends");
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).offset = Wheel { rgb: [0.2, 0.0, -0.2], master: 0.0 };
        assert!(near(one(&g, [0.5, 0.5, 0.5]), [0.6, 0.5, 0.4]), "offset adds, per channel");
    }

    #[test]
    fn contrast_saturation_hue_and_balance() {
        let mut g = NodeGraph::default();
        let c = corrector(&mut g, 1);
        c.contrast = 2.0;
        c.pivot = 0.5;
        assert!(near(one(&g, [0.6, 0.5, 0.4]), [0.7, 0.5, 0.3]));
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).saturation = 0.0;
        let grey = one(&g, [0.8, 0.2, 0.2]);
        assert!(near(grey, [grey[0]; 3]) && (grey[0] - luma([0.8, 0.2, 0.2])).abs() < 1e-4, "{grey:?}");
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).hue = 180.0;
        assert!(near(one(&g, [1.0, 0.0, 0.0]), [0.0, 1.0, 1.0]), "red turned half a circle is cyan");
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).temperature = 1.0;
        let warm = one(&g, [0.5; 3]);
        assert!(warm[0] > 0.5 && warm[2] < 0.5 && (warm[1] - 0.5).abs() < 1e-6, "{warm:?}");
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).highlights = 1.0;
        let lit = one(&g, [0.9; 3]);
        let dark = one(&g, [0.1; 3]);
        assert!(lit[0] - 0.9 > dark[0] - 0.1, "highlights move the bright end more");
    }

    #[test]
    fn curves_and_the_hue_curves() {
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).curves.y = vec![CurvePoint { input: 0.0, output: 0.0 }, CurvePoint { input: 0.5, output: 0.75 }, CurvePoint { input: 1.0, output: 1.0 }];
        let lifted = one(&g, [0.5; 3]);
        assert!((lifted[0] - 0.75).abs() < 0.01, "{lifted:?}");
        // Hue vs Sat: red (hue 0) down to no colour, blue left alone.
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).hue_curves.hue_vs_sat = vec![[0.0, -1.0], [0.1, 0.0], [0.9, 0.0]];
        let red = one(&g, [0.8, 0.2, 0.2]);
        let blue = one(&g, [0.2, 0.2, 0.8]);
        assert!(red[0] - red[1] < 0.05, "red lost its colour: {red:?}");
        assert!(near(blue, [0.2, 0.2, 0.8]), "{blue:?}");
        // A curve wraps round the hue circle: a point at 0.95 reaches hue 0.0 and 1.0 alike.
        let t = curve_table(&[[0.95, 1.0], [0.5, 0.0]], true).unwrap_or_default();
        assert!((table_at(&t, 0.0, true) - table_at(&t, 1.0, true)).abs() < 1e-5);
        assert!(table_at(&t, 0.99, true) > 0.5);
        assert!(curve_table(&[], true).is_none() && curve_table(&[[0.2, 0.0]], false).is_none());
    }

    #[test]
    fn a_qualifier_limits_the_grade_to_a_colour() {
        let mut g = NodeGraph::default();
        let c = corrector(&mut g, 1);
        c.saturation = 0.0;
        c.matte.qualifier = Qualifier { use_hue: true, hue_center: 0.0, hue_width: 0.05, hue_soft: 0.02, ..Qualifier::default() };
        let red = one(&g, [0.8, 0.2, 0.2]);
        let green = one(&g, [0.2, 0.8, 0.2]);
        assert!(near(red, [red[0]; 3]), "the reds went grey: {red:?}");
        assert!(near(green, [0.2, 0.8, 0.2]), "the greens stayed: {green:?}");
        // Inverted, it is the other way round.
        corrector(&mut g, 1).matte.qualifier.invert = true;
        assert!(near(one(&g, [0.8, 0.2, 0.2]), [0.8, 0.2, 0.2]));
        assert!(near(one(&g, [0.2, 0.8, 0.2]), [luma([0.2, 0.8, 0.2]); 3]));
    }

    #[test]
    fn a_window_limits_the_grade_to_a_place() {
        let mut g = NodeGraph::default();
        let c = corrector(&mut g, 1);
        c.offset = Wheel { rgb: [0.0; 3], master: 0.4 };
        c.matte.windows.push(GradeWindow { shape: WindowShape::Circle, center: [0.5, 0.5], size: [0.5, 0.5], softness: 0.0, ..GradeWindow::default() });
        let mut b = buf(40, 40, |_, _| [0.2, 0.2, 0.2, 1.0]);
        apply(&g, &mut b, Rect::new(0, 0, 40, 40));
        let at = |x: usize, y: usize| b.px[y * 40 + x][0];
        assert!((at(20, 20) - 0.4).abs() < 1e-4, "inside: {}", at(20, 20));
        assert!((at(1, 1) - 0.2).abs() < 1e-6, "outside: {}", at(1, 1));
        // A band of the canvas gets the same result as the whole: windows use canvas positions.
        let mut band = buf(40, 10, |_, _| [0.2, 0.2, 0.2, 1.0]);
        band.rect = Rect::new(0, 15, 40, 25);
        apply(&g, &mut band, Rect::new(0, 0, 40, 40));
        assert!((band.px[5 * 40 + 20][0] - at(20, 20)).abs() < 1e-6);
        // Inverted: the other way round; a disabled window is as if absent (the grade shows everywhere).
        corrector(&mut g, 1).matte.windows[0].invert = true;
        let mut b = buf(40, 40, |_, _| [0.2, 0.2, 0.2, 1.0]);
        apply(&g, &mut b, Rect::new(0, 0, 40, 40));
        assert!((b.px[20 * 40 + 20][0] - 0.2).abs() < 1e-6 && (b.px[0][0] - 0.4).abs() < 1e-4);
        corrector(&mut g, 1).matte.windows[0].enabled = false;
        assert!((one(&g, [0.2; 3])[0] - 0.4).abs() < 1e-4);
    }

    #[test]
    fn nodes_run_in_series_and_can_be_bypassed() {
        let mut g = NodeGraph::default();
        let two = g.add_serial(Some(1)).unwrap();
        corrector(&mut g, 1).offset = Wheel { rgb: [0.0; 3], master: 0.2 };
        corrector(&mut g, two).gain = Wheel { rgb: [0.0; 3], master: 1.0 };
        // (0.1 + 0.1) then × 2: the offset wheel is halved (master 0.2 adds 0.1).
        assert!(near(one(&g, [0.1; 3]), [0.4; 3]));
        if let Some(n) = g.node_mut(two) {
            n.bypass = true;
        }
        assert!(near(one(&g, [0.1; 3]), [0.2; 3]));
    }

    #[test]
    fn parallel_branches_add_their_changes_to_the_shared_input() {
        let mut g = NodeGraph::default();
        let b = g.add_parallel(1).unwrap();
        corrector(&mut g, 1).offset = Wheel { rgb: [0.0; 3], master: 0.2 }; // +0.1
        corrector(&mut g, b).offset = Wheel { rgb: [0.0; 3], master: -0.1 }; // -0.05
        assert!(near(one(&g, [0.5; 3]), [0.55; 3]), "{:?}", one(&g, [0.5; 3]));
        // With both neutral the mixer is the identity, not a doubling.
        let mut g = NodeGraph::default();
        g.add_parallel(1).unwrap();
        assert_eq!(one(&g, [0.3, 0.4, 0.5]), [0.3, 0.4, 0.5]);
    }

    #[test]
    fn a_layer_mixer_lays_a_keyed_layer_over_the_base() {
        let mut g = NodeGraph::default();
        let top = g.add_layer(1).unwrap();
        // The top layer takes the red and green out, but only where the source is red.
        let c = corrector(&mut g, top);
        c.gain = Wheel { rgb: [-1.0, -1.0, 0.0], master: 0.0 };
        c.matte.qualifier = Qualifier { use_hue: true, hue_center: 0.0, hue_width: 0.05, ..Qualifier::default() };
        let red = one(&g, [0.8, 0.0, 0.0]);
        let green = one(&g, [0.0, 0.8, 0.0]);
        assert!(near(green, [0.0, 0.8, 0.0]), "outside the key the base shows: {green:?}");
        assert!(red[0] < 0.01, "inside it the top layer shows: {red:?}");
        // At half opacity the top layer is half in.
        let mixer = g.output;
        if let Some(NodeKind::LayerMixer { layers }) = g.node_mut(mixer).map(|n| &mut n.kind) {
            layers[1] = MixLayer { mode: BlendMode::Normal, opacity: 0.5 };
        }
        assert!((one(&g, [0.8, 0.0, 0.0])[0] - 0.4).abs() < 1e-4);
    }

    #[test]
    fn a_lut_node_runs_a_table_at_an_intensity() {
        // An inverting 2³ LUT.
        let mut lut = Vec::new();
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    lut.extend([1.0 - r as f32, 1.0 - g as f32, 1.0 - b as f32]);
                }
            }
        }
        let mut g = NodeGraph::default();
        g.nodes[0].kind = NodeKind::Lut(Box::new(LutNode { name: "invert".into(), lut: Some(std::sync::Arc::new(lut)), size: 2, intensity: 1.0, tetrahedral: false, matte: Matte::default() }));
        assert!(near(one(&g, [0.25, 0.5, 1.0]), [0.75, 0.5, 0.0]));
        if let NodeKind::Lut(l) = &mut g.nodes[0].kind {
            l.intensity = 0.5;
        }
        assert!(near(one(&g, [0.0; 3]), [0.5; 3]));
    }

    #[test]
    fn baking_a_graph_gives_the_same_colours_at_the_grid_points() {
        let mut g = NodeGraph::default();
        let c = corrector(&mut g, 1);
        c.gain = Wheel { rgb: [0.0, 0.0, -0.5], master: 0.0 };
        c.saturation = 1.3;
        let lut = bake_lut(&g, 9).unwrap_or_default();
        assert_eq!(lut.len(), 9 * 9 * 9 * 3);
        for (r, gr, b) in [(0, 0, 0), (8, 8, 8), (8, 0, 4), (2, 5, 7)] {
            let c = [r as f32 / 8.0, gr as f32 / 8.0, b as f32 / 8.0];
            let i = ((b * 9 + gr) * 9 + r) * 3;
            let direct = one(&g, c).map(|v| v.clamp(0.0, 1.0));
            assert!(near([lut[i], lut[i + 1], lut[i + 2]], direct), "{c:?}");
        }
        assert!(!has_windows(&g));
        corrector(&mut g, 1).matte.windows.push(GradeWindow::default());
        assert!(has_windows(&g));
    }

    #[test]
    fn transparent_pixels_and_odd_values_are_left_alone() {
        let mut g = NodeGraph::default();
        corrector(&mut g, 1).offset = Wheel { rgb: [0.0; 3], master: 0.4 };
        let mut b = buf(2, 1, |x, _| if x == 0 { [0.1, 0.1, 0.1, 0.0] } else { [0.1, 0.1, 0.1, 1.0] });
        apply(&g, &mut b, Rect::new(0, 0, 2, 1));
        assert_eq!(b.px[0], [0.1, 0.1, 0.1, 0.0]);
        assert!((b.px[1][0] - 0.3).abs() < 1e-4);
        // NaN settings are read as neutral instead of poisoning the picture.
        let mut g = NodeGraph::default();
        let c = corrector(&mut g, 1);
        c.saturation = f32::NAN;
        c.contrast = f32::INFINITY;
        let out = one(&g, [0.3, 0.4, 0.5]);
        assert!(out.iter().all(|v| v.is_finite()));
    }
}
