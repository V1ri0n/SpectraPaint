//! SVG writer: the document as an SVG 1.1 file in which shape layers stay vector.
//!
//! * A shape layer with a solid fill and/or a centred solid stroke, whose path is made only of
//!   combined components (or the first component combined and the rest excluded, the outline an
//!   image trace gives), is written as a `<path>`.
//! * A group is a `<g>` holding its children.
//! * Every other layer (pixels, type, gradients, shapes with masks, effects, inside/outside
//!   strokes or other path operations) is written as a PNG `<image>` of how it shows on its own,
//!   cropped to its content, and reported as a note.
//!
//! Layer opacity, visibility, names and the CSS-equivalent blend modes are kept. Adjustment layers,
//! clipping and the blend modes SVG has no word for cannot be expressed and are reported.

use std::fmt::Write as _;

use photocraft_codecs::{self as codecs, ChannelLayout, Format, Image, SampleType as CSample};
use photocraft_color::{BlendMode, ColorMode};
use photocraft_doc::{Document, Fill, Layer, LayerContent, LineCap, LineJoin, Path, PathOp, ShapeLayer, ShapeStroke, StrokeAlign};
use photocraft_geom::Rect;
use photocraft_raster::Interrupt;

use crate::ora::Warnings;
use crate::{ExportResult, IoError};

/// Most nested groups written (a hostile document cannot overflow the stack).
const MAX_DEPTH: usize = 64;

fn q8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn hex(c: [f32; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", q8(c[0]), q8(c[1]), q8(c[2]))
}

/// Text safe inside an attribute or element.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

/// A number with no trailing zeros, at most 3 decimals.
fn num(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.into() }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The CSS `mix-blend-mode` of `mode`, `None` for Normal; `Err` when CSS has no such mode.
fn css_blend(mode: BlendMode) -> Result<Option<&'static str>, ()> {
    Ok(Some(match mode {
        BlendMode::Normal | BlendMode::PassThrough => return Ok(None),
        BlendMode::Darken => "darken",
        BlendMode::Multiply => "multiply",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::Lighten => "lighten",
        BlendMode::Screen => "screen",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::Overlay => "overlay",
        BlendMode::SoftLight => "soft-light",
        BlendMode::HardLight => "hard-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
        _ => return Err(()),
    }))
}

/// How the components of `path` combine, if a plain SVG fill can say it: `Some(true)` for the
/// even-odd rule (first component combined, the others excluded), `Some(false)` for the path's own
/// rule when everything combines, `None` when other operations are used.
fn fill_rule(path: &Path) -> Option<bool> {
    if path.inverted {
        return None;
    }
    let rest = path.subpaths.iter().skip(1);
    if path.subpaths.iter().all(|s| s.op == PathOp::Combine) {
        return Some(path.fill_rule == photocraft_doc::FillRule::EvenOdd);
    }
    let first_combines = path.subpaths.first().is_none_or(|s| s.op == PathOp::Combine);
    if first_combines && rest.into_iter().all(|s| s.op == PathOp::Exclude) { Some(true) } else { None }
}

/// The `d` attribute of `path`: each subpath as M, C/L and Z.
fn path_data(path: &Path) -> String {
    let mut d = String::new();
    for sp in &path.subpaths {
        let k = &sp.knots;
        let Some(first) = k.first() else { continue };
        let _ = write!(d, "M{} {}", num(first.anchor.x), num(first.anchor.y));
        let segments = if sp.closed { k.len() } else { k.len() - 1 };
        for i in 0..segments {
            let (a, b) = (&k[i], &k[(i + 1) % k.len()]);
            let straight = a.out_ctrl == a.anchor && b.in_ctrl == b.anchor;
            // `Z` draws a straight closing segment itself.
            if straight && sp.closed && i + 1 == k.len() {
                continue;
            }
            if straight {
                let _ = write!(d, "L{} {}", num(b.anchor.x), num(b.anchor.y));
            } else {
                let _ = write!(d, "C{} {} {} {} {} {}", num(a.out_ctrl.x), num(a.out_ctrl.y), num(b.in_ctrl.x), num(b.in_ctrl.y), num(b.anchor.x), num(b.anchor.y));
            }
        }
        if sp.closed {
            d.push('Z');
        }
    }
    d
}

/// The solid colour and alpha of a fill, if it is one.
fn solid(f: &Fill) -> Option<([f32; 3], f32)> {
    match f {
        Fill::Solid(c) => Some((c.to_rgb(), c.alpha)),
        _ => None,
    }
}

/// The `<path>` attributes of a shape layer, or why it needs pixels instead.
fn shape_attrs(sh: &ShapeLayer) -> Result<String, &'static str> {
    let evenodd = fill_rule(&sh.path).ok_or("shapes combined with path operations other than combine and exclude")?;
    if sh.fill.is_none() && sh.stroke.is_none() {
        return Err("shapes with neither fill nor stroke");
    }
    let mut a = String::new();
    match &sh.fill {
        None => a.push_str(" fill=\"none\""),
        Some(f) => {
            let (c, alpha) = solid(f).ok_or("gradient and pattern fills")?;
            let _ = write!(a, " fill=\"{}\"", hex(c));
            if alpha < 0.999 {
                let _ = write!(a, " fill-opacity=\"{}\"", num(f64::from(alpha)));
            }
        }
    }
    if evenodd {
        a.push_str(" fill-rule=\"evenodd\"");
    }
    if let Some(st) = &sh.stroke {
        stroke_attrs(st, &mut a)?;
    }
    Ok(a)
}

fn stroke_attrs(st: &ShapeStroke, a: &mut String) -> Result<(), &'static str> {
    if st.align != StrokeAlign::Center {
        return Err("strokes that sit inside or outside the outline");
    }
    let (c, alpha) = solid(&st.paint).ok_or("gradient and pattern strokes")?;
    let _ = write!(a, " stroke=\"{}\" stroke-width=\"{}\"", hex(c), num(f64::from(st.width)));
    let opacity = alpha * st.opacity;
    if opacity < 0.999 {
        let _ = write!(a, " stroke-opacity=\"{}\"", num(f64::from(opacity)));
    }
    match st.cap {
        LineCap::Butt => {}
        LineCap::Round => a.push_str(" stroke-linecap=\"round\""),
        LineCap::Square => a.push_str(" stroke-linecap=\"square\""),
    }
    match st.join {
        LineJoin::Miter => {
            if (st.miter_limit - 4.0).abs() > 1e-3 {
                let _ = write!(a, " stroke-miterlimit=\"{}\"", num(f64::from(st.miter_limit.max(1.0))));
            }
        }
        LineJoin::Round => a.push_str(" stroke-linejoin=\"round\""),
        LineJoin::Bevel => a.push_str(" stroke-linejoin=\"bevel\""),
    }
    if !st.dashes.is_empty() {
        // Photoshop's dashes are multiples of the width; SVG's are lengths.
        let w = f64::from(st.width);
        let list: Vec<String> = st.dashes.iter().map(|d| num(f64::from(*d) * w)).collect();
        let _ = write!(a, " stroke-dasharray=\"{}\"", list.join(" "));
        if st.dash_offset != 0.0 {
            let _ = write!(a, " stroke-dashoffset=\"{}\"", num(f64::from(st.dash_offset) * w));
        }
    }
    Ok(())
}

struct Writer<'a> {
    doc: &'a Document,
    out: String,
    warnings: Warnings,
    ctl: &'a Interrupt<'a>,
}

impl Writer<'_> {
    /// Attributes shared by every layer element: id-ish name, opacity, blend, visibility.
    fn common(&mut self, layer: &Layer, extra: &mut String) {
        let _ = write!(extra, " data-name=\"{}\"", esc(&layer.name));
        let opacity = layer.opacity.clamp(0.0, 1.0);
        let mut style = String::new();
        if opacity < 0.999 {
            let _ = write!(style, "opacity:{}", num(f64::from(opacity)));
        }
        match css_blend(layer.blend) {
            Ok(None) => {}
            Ok(Some(m)) => {
                if !style.is_empty() {
                    style.push(';');
                }
                let _ = write!(style, "mix-blend-mode:{m}");
            }
            Err(()) => self.warnings.layer(&layer.name, "has a blend mode SVG has no equivalent for; written as Normal"),
        }
        if !style.is_empty() {
            let _ = write!(extra, " style=\"{style}\"");
        }
        if layer.clipped {
            self.warnings.layer(&layer.name, "is clipped to the layer below; the clipping is not kept");
        }
    }

    /// The layer as a PNG over its content bounds.
    fn image(&mut self, layer: &Layer, why: Option<&str>) -> Result<(), IoError> {
        let canvas = self.doc.bounds();
        let bounds = match photocraft_compose::composite_bounds(layer, canvas) {
            Some(b) => intersect(b, canvas),
            None => canvas,
        };
        if bounds.is_empty() {
            return Ok(());
        }
        let mut solo = layer.clone();
        solo.visible = true;
        solo.opacity = 1.0;
        solo.blend = BlendMode::Normal;
        solo.clipped = false;
        let buf = photocraft_compose::render_layer(&solo, bounds);
        let mut bytes = Vec::with_capacity(buf.px.len() * 4);
        for p in &buf.px {
            bytes.extend_from_slice(&[q8(p[0]), q8(p[1]), q8(p[2]), q8(p[3])]);
        }
        let image = Image::from_raw(bounds.width(), bounds.height(), ChannelLayout::Rgba, CSample::U8, bytes)?;
        let png = codecs::encode(&image, Format::Png, &codecs::EncodeOptions { embed_icc: false, ..Default::default() })?;
        if let Some(why) = why {
            self.warnings.layer(&layer.name, &format!("is {why}, so it is saved as a picture"));
        }
        let mut extra = String::new();
        self.common(layer, &mut extra);
        let _ = writeln!(
            self.out,
            "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{extra} xlink:href=\"data:image/png;base64,{}\"/>",
            bounds.x0,
            bounds.y0,
            bounds.width(),
            bounds.height(),
            base64(&png)
        );
        Ok(())
    }

    fn layers(&mut self, layers: &[Layer], depth: usize) -> Result<(), IoError> {
        for layer in layers.iter().filter(|l| l.visible) {
            self.ctl.check().map_err(|_| IoError::Cancelled)?;
            self.layer(layer, depth)?;
        }
        Ok(())
    }

    fn layer(&mut self, layer: &Layer, depth: usize) -> Result<(), IoError> {
        let plain = layer.mask.is_none() && layer.vector_mask.is_none() && layer.effects.items.is_empty() && layer.effects.psd_raw.is_none() && layer.fill_opacity >= 0.999;
        match &layer.content {
            LayerContent::Group(g) if g.artboard.is_none() && plain && depth < MAX_DEPTH => {
                let mut extra = String::new();
                self.common(layer, &mut extra);
                let _ = writeln!(self.out, "<g{extra}>");
                self.layers(&g.children, depth + 1)?;
                self.out.push_str("</g>\n");
                Ok(())
            }
            LayerContent::Shape(sh) if plain => match shape_attrs(sh) {
                Ok(attrs) => {
                    let mut extra = String::new();
                    self.common(layer, &mut extra);
                    let _ = writeln!(self.out, "<path d=\"{}\"{attrs}{extra}/>", path_data(&sh.path));
                    Ok(())
                }
                Err(why) => self.image(layer, Some(why)),
            },
            LayerContent::Adjustment(_) => {
                self.warnings.layer(&layer.name, "is an adjustment layer; SVG cannot adjust the layers below, so it is left out");
                Ok(())
            }
            LayerContent::Raster(_) => self.image(layer, None),
            LayerContent::Group(_) => self.image(layer, Some("a group with a mask, effects or an artboard")),
            _ if !plain => self.image(layer, Some("a layer with a mask or effects")),
            LayerContent::Shape(_) => self.image(layer, Some("a shape SVG cannot hold")),
            _ => self.image(layer, Some(&format!("{} layer", layer.content.kind_name().to_lowercase()))),
        }
    }
}

fn intersect(a: Rect, b: Rect) -> Rect {
    Rect::new(a.x0.max(b.x0), a.y0.max(b.y0), a.x1.min(b.x1), a.y1.min(b.y1))
}

/// Everything saving `doc` as SVG would approximate or drop, without rendering anything.
pub fn export(doc: &Document, ctl: &Interrupt<'_>) -> Result<ExportResult, IoError> {
    let (w, h) = (doc.size.width, doc.size.height);
    if w == 0 || h == 0 {
        return Err(IoError::Unsupported("an SVG needs a canvas with a size".into()));
    }
    let mut wr = Writer { doc, out: String::new(), warnings: Warnings::default(), ctl };
    if doc.mode != ColorMode::Rgb {
        wr.warnings.note("the document is not RGB; colours are written as their RGB equivalents");
    }
    let _ = writeln!(
        wr.out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">"
    );
    if !doc.name.is_empty() {
        let _ = writeln!(wr.out, "<title>{}</title>", esc(&doc.name));
    }
    wr.layers(&doc.layers, 0)?;
    wr.out.push_str("</svg>\n");
    Ok(ExportResult { bytes: wr.out.into_bytes(), warnings: wr.warnings.finish() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_color::{Color, SampleType};
    use photocraft_doc::{Knot, Subpath};
    use photocraft_geom::{Point, Size};

    fn doc() -> Document {
        Document::new("Test & <doc>", Size::new(40, 30), ColorMode::Rgb, SampleType::U8)
    }

    /// A shape layer with the rendered cache a real document's shape has (the SVG writer takes a
    /// picture of a shape from it).
    fn shape(path: Path, fill: Option<Fill>, stroke: Option<ShapeStroke>) -> Layer {
        let mut sh = ShapeLayer { path, fill, stroke, ..Default::default() };
        let d = doc();
        sh.cache = Some(photocraft_vector::render_shape(&sh, d.pixel_format(), d.bounds()));
        Layer::new("Shape", LayerContent::Shape(sh))
    }

    fn svg(d: &Document) -> (String, Vec<String>) {
        let r = export(d, &Interrupt::NONE).unwrap();
        (String::from_utf8(r.bytes).unwrap(), r.warnings)
    }

    fn triangle() -> Path {
        Path::new(vec![Subpath::polygon(&[(1.0, 2.0), (21.5, 2.0), (1.0, 12.25)])])
    }

    #[test]
    fn numbers_and_escapes() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.23456), "1.235");
        assert_eq!(num(-0.0004), "0");
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(esc("a<b>&\"c\u{1}"), "a&lt;b&gt;&amp;&quot;c");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn a_solid_shape_stays_a_path() {
        let mut d = doc();
        d.layers.push(shape(triangle(), Some(Fill::Solid(Color::rgb(1.0, 0.0, 0.0))), None));
        let (s, warnings) = svg(&d);
        assert!(s.starts_with("<?xml"), "{s}");
        assert!(s.contains("width=\"40\" height=\"30\" viewBox=\"0 0 40 30\""), "{s}");
        assert!(s.contains("<title>Test &amp; &lt;doc&gt;</title>"), "{s}");
        assert!(s.contains("<path d=\"M1 2L21.5 2L1 12.25Z\" fill=\"#ff0000\" data-name=\"Shape\"/>"), "{s}");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!s.contains("<image"), "no pixels for a plain shape");
    }

    #[test]
    fn strokes_curves_dashes_and_opacity() {
        let mut d = doc();
        let curved = Subpath {
            closed: false,
            knots: vec![
                Knot { anchor: Point::new(0.0, 0.0), in_ctrl: Point::new(0.0, 0.0), out_ctrl: Point::new(5.0, 0.0), smooth: false },
                Knot { anchor: Point::new(10.0, 10.0), in_ctrl: Point::new(10.0, 5.0), out_ctrl: Point::new(10.0, 10.0), smooth: false },
            ],
            op: PathOp::Combine,
        };
        let stroke = ShapeStroke {
            width: 2.0,
            paint: Fill::Solid(Color::rgb(0.0, 0.0, 1.0)),
            opacity: 0.5,
            align: StrokeAlign::Center,
            cap: LineCap::Round,
            join: LineJoin::Bevel,
            miter_limit: 4.0,
            dashes: vec![2.0, 1.0],
            dash_offset: 0.5,
            ..Default::default()
        };
        let mut l = shape(Path::new(vec![curved]), None, Some(stroke));
        l.opacity = 0.5;
        d.layers.push(l);
        let (s, warnings) = svg(&d);
        assert!(s.contains("d=\"M0 0C5 0 10 5 10 10\""), "{s}");
        assert!(s.contains("fill=\"none\" stroke=\"#0000ff\" stroke-width=\"2\" stroke-opacity=\"0.5\" stroke-linecap=\"round\" stroke-linejoin=\"bevel\" stroke-dasharray=\"4 2\" stroke-dashoffset=\"1\""), "{s}");
        assert!(s.contains("style=\"opacity:0.5\""), "{s}");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn traced_outlines_use_the_even_odd_rule() {
        let mut hole = Subpath::polygon(&[(3.0, 3.0), (6.0, 3.0), (6.0, 6.0)]);
        hole.op = PathOp::Exclude;
        let outline = Path::new(vec![Subpath::polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]), hole]);
        assert_eq!(fill_rule(&outline), Some(true));
        let mut sub = outline.clone();
        sub.subpaths[1].op = PathOp::Subtract;
        assert_eq!(fill_rule(&sub), None);
        let mut inv = outline;
        inv.inverted = true;
        assert_eq!(fill_rule(&inv), None);
    }

    #[test]
    fn what_svg_cannot_hold_becomes_a_picture_with_a_note() {
        let mut d = doc();
        // A shape with an outside stroke, a gradient, and a raster layer.
        let outside = ShapeStroke { width: 2.0, paint: Fill::Solid(Color::rgb(0.0, 0.0, 0.0)), align: StrokeAlign::Outside, ..Default::default() };
        let mut a = shape(triangle(), Some(Fill::Solid(Color::rgb(0.0, 1.0, 0.0))), Some(outside));
        a.name = "Outlined".into();
        d.layers.push(a);
        let mut multiply = shape(triangle(), Some(Fill::Solid(Color::rgb(1.0, 1.0, 0.0))), None);
        multiply.blend = BlendMode::Multiply;
        d.layers.push(multiply);
        let mut dissolve = shape(triangle(), Some(Fill::Solid(Color::rgb(1.0, 1.0, 0.0))), None);
        dissolve.blend = BlendMode::Dissolve;
        d.layers.push(dissolve);
        let mut hidden = shape(triangle(), Some(Fill::Solid(Color::rgb(0.0, 0.0, 0.0))), None);
        hidden.visible = false;
        hidden.name = "Hidden".into();
        d.layers.push(hidden);
        let (s, warnings) = svg(&d);
        assert!(s.contains("<image"), "{s}");
        assert!(s.contains("data:image/png;base64,iVBOR"), "a PNG");
        assert!(s.contains("mix-blend-mode:multiply"), "{s}");
        assert!(!s.contains("Hidden"), "hidden layers are left out");
        assert!(warnings.iter().any(|w| w.contains("Outlined") && w.contains("picture")), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("blend mode")), "{warnings:?}");
    }

    #[test]
    fn groups_nest_and_adjustments_are_reported() {
        let mut d = doc();
        let inner = shape(triangle(), Some(Fill::Solid(Color::rgb(0.0, 0.0, 0.0))), None);
        d.layers.push(Layer::group("Group", vec![inner]));
        let (s, w) = svg(&d);
        assert!(s.contains("<g data-name=\"Group\">\n<path"), "{s}");
        assert!(s.contains("</g>"), "{s}");
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn an_empty_canvas_is_refused() {
        let d = Document::new("x", Size::new(0, 0), ColorMode::Rgb, SampleType::U8);
        assert!(export(&d, &Interrupt::NONE).is_err());
    }
}
