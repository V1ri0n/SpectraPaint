//! Image Trace (`path.imageTrace`): turns a picture into vector shape layers, one per colour.
//!
//! 1. The source (a layer, or the whole image) is rendered over the canvas; transparent pixels
//!    are left out.
//! 2. Its colours are reduced to a palette: black and white by a threshold, grey levels, or `colors`
//!    colours chosen by median cut.
//! 3. The palette is ordered from the largest area to the smallest. Colour *k* is traced from every
//!    pixel of rank *k* or later and the shapes are stacked in that order, so each shape reaches
//!    under its neighbours and no seams or gaps open between them (Illustrator's "stacked" method).
//! 4. Each mask is traced to Bézier outlines (`photocraft_vector::trace`), specks smaller than
//!    `minArea` pixels are dropped, and the shapes go into a new group above the source.

use photocraft_color::Color;
use photocraft_doc::{Fill, Layer, LayerContent, PathOp, ShapeLayer};
use photocraft_geom::Rect;
use photocraft_vector as vector;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

/// Most colours a trace has.
pub const MAX_COLORS: usize = 32;
/// Largest picture traced, in pixels (a trace allocates a mask per colour).
const MAX_PIXELS: u64 = 16_000_000;
/// Colours sampled to build the palette (the rest are only matched to it).
const MAX_SAMPLES: usize = 200_000;

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "path.imageTrace".into(), msg: msg.into() }
}

fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// `colors` colours chosen by median cut over `samples`: the box with the widest channel range is
/// split at the median of that channel until there are enough boxes; each box gives its mean.
fn median_cut(mut samples: Vec<[f32; 3]>, colors: usize) -> Vec<[f32; 3]> {
    if samples.is_empty() {
        return Vec::new();
    }
    let range = |b: &[[f32; 3]]| -> (usize, f32) {
        (0..3)
            .map(|ch| {
                let (lo, hi) = b.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[ch]), hi.max(p[ch])));
                (ch, hi - lo)
            })
            .fold((0, -1.0), |best, c| if c.1 > best.1 { c } else { best })
    };
    // Boxes as ranges of `samples`, which is reordered in place as they are split.
    let mut boxes: Vec<(usize, usize)> = vec![(0, samples.len())];
    while boxes.len() < colors {
        // The box with the widest range that can still be split.
        let pick = boxes.iter().enumerate().filter(|(_, (a, b))| b - a > 1).map(|(i, &(a, b))| (i, range(&samples[a..b]))).max_by(|x, y| x.1.1.total_cmp(&y.1.1));
        let Some((i, (ch, width))) = pick else { break };
        if width <= 1e-4 {
            break;
        }
        let (a, b) = boxes[i];
        samples[a..b].sort_unstable_by(|x, y| x[ch].total_cmp(&y[ch]));
        let mid = a + (b - a) / 2;
        boxes[i] = (a, mid);
        boxes.push((mid, b));
    }
    boxes
        .into_iter()
        .map(|(a, b)| {
            let n = (b - a) as f32;
            let sum = samples[a..b].iter().fold([0.0f32; 3], |acc, p| [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]]);
            [sum[0] / n, sum[1] / n, sum[2] / n]
        })
        .collect()
}

/// The palette for `mode` over the opaque pixels of `px`.
fn palette(px: &[[f32; 4]], mode: &str, colors: usize, threshold: f32) -> Vec<[f32; 3]> {
    match mode {
        "bw" => vec![[0.0; 3], [1.0; 3]],
        "grayscale" => (0..colors).map(|i| [i as f32 / (colors - 1) as f32; 3]).collect(),
        _ => {
            let opaque = px.iter().filter(|p| p[3] >= threshold).count().max(1);
            let step = (opaque / MAX_SAMPLES).max(1);
            let samples: Vec<[f32; 3]> = px.iter().filter(|p| p[3] >= threshold).step_by(step).map(|p| [p[0], p[1], p[2]]).collect();
            median_cut(samples, colors)
        }
    }
}

/// Index of the palette colour nearest to `c` (squared distance in RGB; for "bw" by brightness).
fn nearest(pal: &[[f32; 3]], c: [f32; 3], by_luma: bool) -> usize {
    let d = |p: &[f32; 3]| if by_luma { (luma(*p) - luma(c)).abs() } else { (0..3).map(|i| (p[i] - c[i]) * (p[i] - c[i])).sum::<f32>() };
    pal.iter().enumerate().min_by(|a, b| d(a.1).total_cmp(&d(b.1))).map_or(0, |(i, _)| i)
}

/// Twice the signed area of a subpath's anchor polygon (a size test for specks, not exact).
fn area2(sp: &photocraft_doc::Subpath) -> f64 {
    let k = &sp.knots;
    (0..k.len()).map(|i| (k[i].anchor.x * k[(i + 1) % k.len()].anchor.y) - (k[(i + 1) % k.len()].anchor.x * k[i].anchor.y)).sum()
}

/// The traced layers' colours and outlines for the pixels `px` over `rect`.
struct Traced {
    color: [f32; 3],
    path: photocraft_doc::Path,
    area: usize,
}

/// What a trace is asked for.
struct Options<'a> {
    mode: &'a str,
    colors: usize,
    /// Pixels with less alpha than this are not traced.
    threshold: f32,
    tolerance: f64,
    min_area: f64,
    ignore_white: bool,
}

/// Quantises `px` and traces the stacked masks. Returns the shapes bottom to top.
fn trace(px: &[[f32; 4]], rect: Rect, o: &Options) -> Vec<Traced> {
    let Options { mode, colors, threshold, tolerance, min_area, ignore_white } = *o;
    if px.is_empty() || rect.is_empty() {
        return Vec::new();
    }
    let mut pal = palette(px, mode, colors, threshold);
    if pal.is_empty() {
        return Vec::new();
    }
    let by_luma = mode == "bw";
    // Nearest palette colour per pixel (a cache by 5-bit colour keeps big images quick).
    let mut cache = vec![u16::MAX; 32 * 32 * 32];
    let mut which: Vec<u16> = Vec::with_capacity(px.len());
    let mut area = vec![0usize; pal.len()];
    for p in px {
        if p[3] < threshold {
            which.push(u16::MAX);
            continue;
        }
        let key = {
            let q = |v: f32| ((v.clamp(0.0, 1.0) * 31.0 + 0.5) as usize).min(31);
            (q(p[0]) << 10) | (q(p[1]) << 5) | q(p[2])
        };
        if cache[key] == u16::MAX {
            cache[key] = nearest(&pal, [p[0], p[1], p[2]], by_luma) as u16;
        }
        area[cache[key] as usize] += 1;
        which.push(cache[key]);
    }
    // Largest area first; colours that never occur are dropped.
    let mut order: Vec<usize> = (0..pal.len()).filter(|i| area[*i] > 0).collect();
    order.sort_by(|a, b| area[*b].cmp(&area[*a]));
    let rank_of: Vec<usize> = {
        let mut r = vec![usize::MAX; pal.len()];
        for (rank, i) in order.iter().enumerate() {
            r[*i] = rank;
        }
        r
    };
    // The mask reaches one pixel past the picture, repeating its edge pixels, so a shape that
    // touches the edge is traced beyond it: the tracer rounds corners and would otherwise leave
    // the picture's own corners and edges a little uncovered.
    let (w, h) = (rect.width() as usize, rect.height() as usize);
    let wide = Rect::new(rect.x0 - 1, rect.y0 - 1, rect.x1 + 1, rect.y1 + 1);
    let padded: Vec<usize> = (0..h + 2).flat_map(|y| (0..w + 2).map(move |x| (y.saturating_sub(1).min(h - 1)) * w + x.saturating_sub(1).min(w - 1))).collect();
    let mut out = Vec::new();
    for (rank, &i) in order.iter().enumerate() {
        let c = std::mem::take(&mut pal[i]);
        // White ("paper") is left out when asked, unless it is the only colour.
        if ignore_white && luma(c) > 0.97 && c.iter().all(|v| *v > 0.94) && order.len() > 1 {
            continue;
        }
        let mask: Vec<f32> = padded.iter().map(|&at| if which[at] != u16::MAX && rank_of[which[at] as usize] >= rank { 1.0 } else { 0.0 }).collect();
        let mut path = vector::trace::trace_mask(&mask, wide, 0.0, tolerance);
        if min_area > 0.0 {
            path.subpaths.retain(|sp| area2(sp).abs() / 2.0 >= min_area);
            // The first subpath combines, the rest exclude: keep that after dropping one.
            if let Some(first) = path.subpaths.first_mut() {
                first.op = PathOp::Combine;
            }
        }
        if path.is_empty() {
            continue;
        }
        out.push(Traced { color: c, path, area: area[i] });
    }
    out
}

fn image_trace(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = p.get("mode").and_then(Value::as_str).unwrap_or("color");
    if !["color", "bw", "grayscale"].contains(&mode) {
        return Err(bad(format!("unknown `mode` `{mode}` (color|bw|grayscale)")));
    }
    let colors = match p.get("colors") {
        None | Some(Value::Null) => 8,
        Some(v) => v.as_u64().filter(|n| (2..=MAX_COLORS as u64).contains(n)).ok_or_else(|| bad(format!("`colors` is 2..{MAX_COLORS}")))? as usize,
    };
    let num = |k: &str, lo: f64, hi: f64, d: f64| -> Result<f64> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(d),
            Some(v) => v.as_f64().filter(|x| x.is_finite() && (lo..=hi).contains(x)).ok_or_else(|| bad(format!("`{k}` is {lo}..{hi}"))),
        }
    };
    let tolerance = num("tolerance", 0.1, 10.0, 1.0)?;
    let min_area = num("minArea", 0.0, 1_000_000.0, 4.0)?;
    let threshold = num("alphaThreshold", 0.01, 1.0, 0.5)? as f32;
    let ignore_white = p.get("ignoreWhite").and_then(Value::as_bool).unwrap_or(false);
    let whole = match p.get("source").and_then(Value::as_str).unwrap_or("layer") {
        "layer" => false,
        "image" => true,
        other => return Err(bad(format!("unknown `source` `{other}` (layer|image)"))),
    };
    let target = if whole { None } else { Some(crate::commands::layer_param(s, p)?) };
    let (rect, px) = {
        let d = s.active().ok_or(EngineError::NoDocument)?;
        let rect = d.doc.bounds();
        if u64::from(rect.width()) * u64::from(rect.height()) > MAX_PIXELS {
            return Err(EngineError::Other(format!("the image is too large to trace (over {MAX_PIXELS} pixels)")));
        }
        let buf = match target {
            None => photocraft_compose::flatten(&d.doc),
            Some(id) => photocraft_compose::render_layer(d.doc.layer(id).ok_or(EngineError::NoLayer(id))?, rect),
        };
        (rect, buf.px)
    };
    let shapes = trace(&px, rect, &Options { mode, colors, threshold, tolerance, min_area, ignore_white });
    if shapes.is_empty() {
        return Err(EngineError::Other("nothing to trace (the source is empty)".into()));
    }
    let count = shapes.len();
    let name = p.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()).map(|n| n.trim().to_string());
    let (group, areas) = s.edit("Image Trace", |doc, active| {
        let mut layers = Vec::with_capacity(shapes.len());
        let mut areas = Vec::new();
        for t in shapes {
            let color = Color::rgba(t.color[0], t.color[1], t.color[2], 1.0);
            let mut sh = ShapeLayer { path: t.path, fill: Some(Fill::Solid(color)), ..Default::default() };
            crate::vector_cmds::refresh_shape(doc, &mut sh);
            areas.push(t.area);
            layers.push(Layer::new(crate::presets::swatches::hex(t.color), LayerContent::Shape(sh)));
        }
        let name = name.unwrap_or_else(|| doc.next_layer_name("Image Trace"));
        let group = Layer::group(name, layers);
        let id = doc.insert_above(target.or(*active), group);
        *active = Some(id);
        Ok((id, areas))
    })?;
    Ok(json!({"layer": group.0, "shapes": count, "areas": areas}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "path.imageTrace",
        label: "Image Trace…",
        menu: &["Layer"],
        shortcut: None,
        params: r##"{"source":"layer|image"="layer" (the layer, or the whole picture),"layer":id?,"mode":"color|bw|grayscale"="color","colors":2..32=8,"tolerance":0.1..10=1 (px),"minArea":px²=4 (smaller specks are dropped),"ignoreWhite":bool=false,"alphaThreshold":0.01..1=0.5,"name":str?} → {layer (the new group),shapes,areas}"##,
        enabled: has_doc,
        run: image_trace,
        journal: true,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(mode: &str, colors: usize, min_area: f64, ignore_white: bool) -> Options<'_> {
        Options { mode, colors, threshold: 0.5, tolerance: 1.0, min_area, ignore_white }
    }

    fn px_of(w: usize, h: usize, f: impl Fn(usize, usize) -> [f32; 4]) -> Vec<[f32; 4]> {
        (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| f(x, y)).collect()
    }

    #[test]
    fn median_cut_finds_the_main_colours() {
        let mut samples = vec![[1.0, 0.0, 0.0]; 50];
        samples.extend(vec![[0.0, 0.0, 1.0]; 50]);
        let mut pal = median_cut(samples, 2);
        pal.sort_by(|a, b| a[0].total_cmp(&b[0]));
        assert_eq!(pal, vec![[0.0, 0.0, 1.0], [1.0, 0.0, 0.0]]);
        // One colour cannot be split.
        assert_eq!(median_cut(vec![[0.3; 3]; 10], 8).len(), 1);
        assert!(median_cut(Vec::new(), 4).is_empty());
    }

    #[test]
    fn stacked_masks_leave_no_gap() {
        let rect = Rect::new(0, 0, 20, 10);
        // Left half red, right half blue, on opaque pixels.
        let px = px_of(20, 10, |x, _| if x < 12 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] });
        let shapes = trace(&px, rect, &opts("color", 2, 0.0, false));
        assert_eq!(shapes.len(), 2);
        // The larger colour (red) is the bottom shape and covers the whole picture.
        assert_eq!(shapes[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(shapes[0].area, 120);
        let bottom = vector::fill_rasterizer(&shapes[0].path, vector::DEFAULT_TOLERANCE).render(rect);
        let worst = bottom.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).map(|(i, v)| (i % 20, i / 20, *v));
        assert!(bottom.iter().all(|v| *v > 0.9), "the bottom shape is the full rectangle; least covered pixel (x, y, coverage): {worst:?}");
        let top = vector::fill_rasterizer(&shapes[1].path, vector::DEFAULT_TOLERANCE).render(rect);
        assert!(top[19] > 0.9 && top[0] < 0.1, "the top shape is the blue part only");
    }

    #[test]
    fn transparent_pixels_and_white_are_skipped() {
        let rect = Rect::new(0, 0, 10, 10);
        let px = px_of(10, 10, |x, _| if x < 5 { [0.0, 0.0, 0.0, 0.0] } else { [0.0, 0.0, 0.0, 1.0] });
        let shapes = trace(&px, rect, &opts("bw", 2, 0.0, false));
        assert_eq!(shapes.len(), 1, "only the opaque half is traced");
        let white_and_ink = px_of(10, 10, |x, _| if x < 7 { [1.0; 4] } else { [0.0, 0.0, 0.0, 1.0] });
        assert_eq!(trace(&white_and_ink, rect, &opts("bw", 2, 0.0, false)).len(), 2);
        let ink_only = trace(&white_and_ink, rect, &opts("bw", 2, 0.0, true));
        assert_eq!(ink_only.len(), 1);
        assert_eq!(ink_only[0].color, [0.0; 3]);
    }

    #[test]
    fn specks_below_min_area_are_dropped() {
        let rect = Rect::new(0, 0, 30, 30);
        // A 10×10 square and a 1-pixel dot, black on white.
        let px = px_of(30, 30, |x, y| if (5..15).contains(&x) && (5..15).contains(&y) || (x, y) == (25, 25) { [0.0, 0.0, 0.0, 1.0] } else { [1.0; 4] });
        let all = trace(&px, rect, &opts("bw", 2, 0.0, true));
        let clean = trace(&px, rect, &opts("bw", 2, 4.0, true));
        assert_eq!(all[0].path.subpaths.len(), 2);
        assert_eq!(clean[0].path.subpaths.len(), 1);
    }

    #[test]
    fn command_makes_a_group_of_shapes_and_undoes() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 60, "height": 40, "background": "white"})).unwrap();
        s.execute("shape.create", json!({"kind": "rect", "rect": [5, 5, 20, 20], "fill": "#ff0000", "name": "Red"})).unwrap();
        s.execute("shape.create", json!({"kind": "ellipse", "rect": [30, 10, 25, 25], "fill": "#0000ff", "name": "Blue"})).unwrap();
        let layers = s.active().unwrap().doc.layer_count();
        let r = s.execute("path.imageTrace", json!({"source": "image", "colors": 3})).unwrap();
        assert_eq!(r["shapes"], 3, "white, red and blue: {r}");
        let id = photocraft_doc::LayerId(r["layer"].as_u64().unwrap());
        let d = &s.active().unwrap().doc;
        let g = d.layer(id).unwrap();
        assert!(g.is_group());
        assert_eq!(g.children().map(<[_]>::len), Some(3));
        assert_eq!(d.layer_count(), layers + 4, "the group and its three shapes");
        assert!(g.children().unwrap().iter().all(|l| matches!(l.content, LayerContent::Shape(_))));
        assert_eq!(s.active().unwrap().active_layer, Some(id));
        // The traced shapes are plain solid paths, so the SVG export keeps them vector, and
        // rendering that SVG again gives a picture of the same size.
        let (bytes, notes) = crate::file_cmds::encode(&s.active().unwrap().doc, "traced.svg", None::<f64>).unwrap();
        let svg = String::from_utf8(bytes.clone()).unwrap();
        assert!(svg.matches("<path").count() >= 3, "{svg}");
        assert!(notes.is_empty(), "{notes:?}");
        let back = photocraft_io::svg::rasterize(&bytes, 1.0).unwrap();
        assert_eq!((back.rect.width(), back.rect.height()), (60, 40));
        assert!(s.undo());
        assert_eq!(s.active().unwrap().doc.layer_count(), layers);
        // Bad input is refused.
        assert!(s.execute("path.imageTrace", json!({"mode": "sepia"})).is_err());
        assert!(s.execute("path.imageTrace", json!({"colors": 1})).is_err());
        assert!(s.execute("path.imageTrace", json!({"tolerance": 0})).is_err());
        assert!(s.execute("path.imageTrace", json!({"source": "nowhere"})).is_err());
    }
}
