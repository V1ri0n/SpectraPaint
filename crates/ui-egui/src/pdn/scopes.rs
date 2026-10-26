//! The two scopes of a colourist's desk, each in its own window: the RGB parade (the picture's
//! columns across, each channel's level up, the three channels side by side) and the vectorscope
//! (every pixel's colour as a point on the Cb/Cr plane, BT.709, with the six colour targets of 75 %
//! bars and the skin-tone line). They read the composite of the active document, reduced to at most
//! 256 pixels on its longer side, and follow every change of it.
//!
//! The look follows a screenshot of DaVinci Resolve's scopes the user sent: black ground, a gold
//! graticule (the parade's lines at every 128 of 1023, the vectorscope's ring with its ticks, the
//! cross, the skin-tone line and the corner-bracket targets), and a soft trace that glows white
//! where it is dense. The drawing itself is this program's own; the tick spacing and the glow are
//! read off the picture by eye, so they are close to Resolve's, not exact.

use super::grade_ui;
use crate::PhotocraftApp;
use egui::{Color32, ColorImage, Context, Id, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Ui, pos2, vec2};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Columns of the parade per channel, and levels: the grid is 256 × 256 for each channel.
const GRID: usize = 256;
/// The longest side of the picture the scopes read.
const READ_SIDE: u32 = 256;
/// The Cb/Cr extent of the vectorscope's ring: the 75 % targets sit at about 0.58 of its radius, as in Resolve.
const REACH: f32 = 0.66;

/// What the scopes counted in one picture.
pub struct Counts {
    /// `[channel][level * GRID + column]`.
    pub parade: Vec<u32>,
    /// `[cr_cell * GRID + cb_cell]`.
    pub plane: Vec<u32>,
    /// The sum of the colours that fell in each cell of the plane.
    pub plane_rgb: Vec<[f32; 3]>,
    pub samples: u64,
}

impl Counts {
    fn empty() -> Counts {
        Counts { parade: vec![0; 3 * GRID * GRID], plane: vec![0; GRID * GRID], plane_rgb: vec![[0.0; 3]; GRID * GRID], samples: 0 }
    }
}

/// Cb and Cr of a colour (BT.709), -0.5..=0.5 for colours in 0..=1.
pub fn cb_cr([r, g, b]: [f32; 3]) -> (f32, f32) {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    ((b - y) / 1.8556, (r - y) / 1.5748)
}

fn cell(v: f32) -> usize {
    ((v.clamp(0.0, 1.0) * (GRID - 1) as f32).round() as usize).min(GRID - 1)
}

/// Counts the `w`×`h` picture `px` (straight-alpha floats; what has no alpha is not counted).
pub fn count(px: &[[f32; 4]], w: usize, h: usize) -> Counts {
    let mut c = Counts::empty();
    if w == 0 || h == 0 {
        return c;
    }
    for (i, p) in px.iter().enumerate().take(w * h) {
        if p[3] <= 0.0 || !p.iter().all(|v| v.is_finite()) {
            continue;
        }
        let col = ((i % w) * GRID / w).min(GRID - 1);
        let rgb = [p[0].clamp(0.0, 1.0), p[1].clamp(0.0, 1.0), p[2].clamp(0.0, 1.0)];
        for (ch, v) in rgb.iter().enumerate() {
            if let Some(n) = c.parade.get_mut(ch * GRID * GRID + cell(*v) * GRID + col) {
                *n = n.saturating_add(1);
            }
        }
        let (cb, cr) = cb_cr(rgb);
        let at = cell(cr / REACH / 2.0 + 0.5) * GRID + cell(cb / REACH / 2.0 + 0.5);
        if let (Some(n), Some(s)) = (c.plane.get_mut(at), c.plane_rgb.get_mut(at)) {
            *n = n.saturating_add(1);
            for k in 0..3 {
                s[k] += rgb[k];
            }
        }
        c.samples += 1;
    }
    c
}

/// How bright a cell holding `n` pixels draws: on a log scale, so one pixel shows (about a fifth)
/// and a crowd of thirty saturates.
fn glow(n: f32, gain: f32) -> f32 {
    if n <= 0.0 { 0.0 } else { ((1.0 + n * gain).ln() / 33.0f32.ln()).clamp(0.0, 1.0) }
}

/// The counts a little softened, as the scopes' traces are: most of a cell stays, the rest spreads
/// to its neighbours (a 3×3 binomial kernel).
fn soften(d: &[f32], w: usize, h: usize) -> Vec<f32> {
    let at = |x: isize, y: isize| -> f32 {
        if x < 0 || y < 0 || x >= w as isize || y >= h as isize { 0.0 } else { d.get(y as usize * w + x as usize).copied().unwrap_or(0.0) }
    };
    let mut out = vec![0.0; w * h];
    for y in 0..h as isize {
        for x in 0..w as isize {
            let centre = at(x, y);
            let blur = (4.0 * centre
                + 2.0 * (at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1))
                + (at(x - 1, y - 1) + at(x + 1, y - 1) + at(x - 1, y + 1) + at(x + 1, y + 1)))
                / 16.0;
            if let Some(o) = out.get_mut(y as usize * w + x as usize) {
                *o = 0.6 * centre + 0.4 * blur;
            }
        }
    }
    out
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn rgb8(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8)
}

/// The parade as one image: the three channels side by side, level 1 at the top.
pub fn parade_image(c: &Counts, gain: f32) -> ColorImage {
    let mut pixels = vec![Color32::BLACK; 3 * GRID * GRID];
    let tint = [[0.92, 0.22, 0.2], [0.2, 0.86, 0.27], [0.25, 0.36, 0.97]];
    for (ch, tint) in tint.iter().enumerate() {
        let plane: Vec<f32> = c.parade.get(ch * GRID * GRID..(ch + 1) * GRID * GRID).unwrap_or_default().iter().map(|&n| n as f32).collect();
        let plane = soften(&plane, GRID, GRID);
        for level in 0..GRID {
            for col in 0..GRID {
                let g = glow(plane.get(level * GRID + col).copied().unwrap_or(0.0), gain);
                if g <= 0.0 {
                    continue;
                }
                if let Some(px) = pixels.get_mut((GRID - 1 - level) * 3 * GRID + ch * GRID + col) {
                    // A dense trace is a little paler, as on a screen.
                    *px = rgb8(mix(tint.map(|v| v * g), [g; 3], g * g * 0.25));
                }
            }
        }
    }
    ColorImage::new([3 * GRID, GRID], pixels)
}

/// The vectorscope plane as an image (Cr up): each cell in the colour its pixels had, white
/// where it is dense.
pub fn plane_image(c: &Counts, gain: f32) -> ColorImage {
    let mut pixels = vec![Color32::BLACK; GRID * GRID];
    let count: Vec<f32> = c.plane.iter().map(|&n| n as f32).collect();
    let soft = soften(&count, GRID, GRID);
    for row in 0..GRID {
        for col in 0..GRID {
            let i = row * GRID + col;
            let g = glow(soft.get(i).copied().unwrap_or(0.0), gain);
            if g <= 0.0 {
                continue;
            }
            let n = c.plane.get(i).copied().unwrap_or(0);
            let mut rgb = c.plane_rgb.get(i).copied().unwrap_or([0.0; 3]);
            if n > 0 {
                rgb = rgb.map(|v| v / n as f32);
            } else {
                // A cell only its neighbours reach: a pale colour.
                rgb = [0.6; 3];
            }
            // The hue at full strength, so a dim colour is still readable; the count sets the glow.
            let top = rgb.iter().copied().fold(0.0f32, f32::max);
            rgb = if top > 1e-4 { rgb.map(|v| v / top) } else { [0.8; 3] };
            if let Some(px) = pixels.get_mut((GRID - 1 - row) * GRID + col) {
                *px = rgb8(mix(rgb.map(|v| v * g), [1.0; 3], g * g * g * 0.9));
            }
        }
    }
    ColorImage::new([GRID, GRID], pixels)
}

// ---------------------------------------------------------------------------------------------
// What the windows share

#[derive(Default)]
struct Cache {
    /// The document and revision the counts are of.
    key: Option<(usize, u64)>,
    counts: Option<Arc<Counts>>,
    at: Option<Instant>,
    cost: Duration,
    /// Bumps when the counts do, so the images are rebuilt.
    generation: u64,
}

type Shared = Arc<Mutex<Cache>>;

fn shared(ctx: &Context) -> Shared {
    let id = Id::new("pdn-scopes-cache");
    if let Some(s) = ctx.data(|d| d.get_temp::<Shared>(id)) {
        return s;
    }
    let s: Shared = Arc::default();
    ctx.data_mut(|d| d.insert_temp(id, s.clone()));
    s
}

/// The counts of the active document's present state, recounted when it has changed. A document
/// that takes long to compose is read less often (a few times its own cost apart), so working on it
/// stays smooth and the scopes catch up when the hand rests.
fn current(app: &PhotocraftApp, ctx: &Context) -> Option<(Arc<Counts>, u64)> {
    let cache = shared(ctx);
    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(s) = app.session.active() else {
        *cache = Cache::default();
        return None;
    };
    let key = (Arc::as_ptr(&s.doc) as usize, s.revision);
    if cache.key != Some(key) {
        let wait = cache.cost * 3;
        let due = cache.at.is_none_or(|at| at.elapsed() >= wait);
        if cache.counts.is_none() || due {
            let start = Instant::now();
            let buf = photocraft_compose::thumbnail_buffer(&s.doc, READ_SIDE);
            let (w, h) = (buf.rect.width().max(0) as usize, buf.rect.height().max(0) as usize);
            cache.counts = Some(Arc::new(count(&buf.px, w, h)));
            cache.key = Some(key);
            cache.cost = start.elapsed();
            cache.at = Some(Instant::now());
            cache.generation += 1;
        } else {
            // Not yet: look again when the wait is over.
            ctx.request_repaint_after(wait.saturating_sub(cache.at.map_or(Duration::ZERO, |at| at.elapsed())).max(Duration::from_millis(16)));
        }
    }
    cache.counts.clone().map(|c| (c, cache.generation))
}

/// The scope's image as a texture, rebuilt when the counts or the gain moved.
fn texture(ctx: &Context, name: &str, generation: u64, gain: f32, make: impl FnOnce() -> ColorImage) -> TextureHandle {
    let id = Id::new(("pdn-scopes-texture", name));
    let stamp = (generation, gain.to_bits());
    if let Some((t, s)) = ctx.data(|d| d.get_temp::<(TextureHandle, (u64, u32))>(id))
        && s == stamp
    {
        return t;
    }
    let t = ctx.load_texture(name, make(), TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, (t.clone(), stamp)));
    t
}

fn gain_row(ui: &mut Ui, key: &str) -> f32 {
    let id = Id::new(("pdn-scopes-gain", key));
    let mut gain = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(1.0);
    if let Some(v) = grade_ui::slider(ui, grade_ui::tr("Яркость", "Brightness"), gain, (0.1, 5.0), 1.0, 1) {
        gain = v;
        ui.ctx().data_mut(|d| d.insert_temp(id, gain));
    }
    gain
}

/// The graticule's gold, at full and at half strength.
const GOLD: Color32 = Color32::from_rgb(160, 136, 38);
const GOLD_DIM: Color32 = Color32::from_rgb(98, 84, 26);
const GOLD_TEXT: Color32 = Color32::from_rgb(176, 150, 46);

fn line(ui: &Ui, a: Pos2, b: Pos2, colour: Color32) {
    ui.painter().line_segment([a, b], Stroke::new(1.0, colour));
}

fn none_to_show(ui: &mut Ui) {
    ui.label(grade_ui::tr("Нет открытого документа.", "No document is open."));
}

// ---------------------------------------------------------------------------------------------
// RGB parade

/// The scale's codes, as Resolve's 10-bit scopes number them.
const PARADE_CODES: [u32; 9] = [0, 128, 256, 384, 512, 640, 768, 896, 1023];

pub fn parade_body(app: &mut PhotocraftApp, ui: &mut Ui) {
    let gain = gain_row(ui, "parade");
    let Some((counts, generation)) = current(app, ui.ctx()) else { return none_to_show(ui) };
    let avail = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(vec2(avail.x.max(80.0), avail.y.max(80.0)), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    // A margin on the left for the codes and one below for the ruler.
    let plot = Rect::from_min_max(pos2(rect.left() + 36.0, rect.top() + 8.0), pos2(rect.right() - 6.0, rect.bottom() - 14.0));
    let tex = texture(ui.ctx(), "pdn-parade", generation, gain, || parade_image(&counts, gain));
    ui.painter().image(tex.id(), plot, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    for code in PARADE_CODES {
        let y = plot.bottom() - plot.height() * code as f32 / 1023.0;
        let strong = matches!(code, 0 | 256 | 512 | 768 | 1023);
        line(ui, pos2(plot.left(), y), pos2(plot.right(), y), if strong { GOLD } else { GOLD_DIM });
        ui.painter().text(pos2(plot.left() - 5.0, y), egui::Align2::RIGHT_CENTER, code.to_string(), egui::FontId::proportional(9.5), GOLD_TEXT);
    }
    // The ruler along the bottom.
    for i in 0..=48 {
        let x = plot.left() + plot.width() * i as f32 / 48.0;
        let len = if i % 4 == 0 { 6.0 } else { 3.0 };
        line(ui, pos2(x, plot.bottom() + 2.0), pos2(x, plot.bottom() + 2.0 + len), GOLD_DIM);
    }
}

// ---------------------------------------------------------------------------------------------
// Vectorscope

/// The six colour targets, where 75 % bars land.
const TARGETS: [(&str, [f32; 3]); 6] = [("R", [1.0, 0.0, 0.0]), ("Y", [1.0, 1.0, 0.0]), ("G", [0.0, 1.0, 0.0]), ("C", [0.0, 1.0, 1.0]), ("B", [0.0, 0.0, 1.0]), ("M", [1.0, 0.0, 1.0])];
/// The skin-tone line, in degrees counter-clockwise from the +Cb axis.
const SKIN_DEGREES: f32 = 123.0;

/// Where a Cb/Cr pair is in a square `plot` (Cr up).
pub fn plane_point(plot: Rect, cb: f32, cr: f32) -> Pos2 {
    let half = plot.width().min(plot.height()) / 2.0;
    pos2(plot.center().x + cb / REACH * half, plot.center().y - cr / REACH * half)
}

/// Four corner brackets round `centre`, as Resolve marks a target.
fn bracket(ui: &Ui, centre: Pos2, half: f32, colour: Color32) {
    let arm = half * 0.7;
    for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let corner = pos2(centre.x + sx * half, centre.y + sy * half);
        line(ui, corner, pos2(corner.x - sx * arm, corner.y), colour);
        line(ui, corner, pos2(corner.x, corner.y - sy * arm), colour);
    }
}

pub fn vectorscope_body(app: &mut PhotocraftApp, ui: &mut Ui) {
    let gain = gain_row(ui, "vectorscope");
    let Some((counts, generation)) = current(app, ui.ctx()) else { return none_to_show(ui) };
    let avail = ui.available_size();
    let side = avail.x.min(avail.y).max(80.0);
    let (outer, _) = ui.allocate_exact_size(vec2(avail.x.max(80.0), avail.y.max(80.0)), Sense::hover());
    ui.painter().rect_filled(outer, 0.0, Color32::BLACK);
    let plot = Rect::from_center_size(outer.center(), vec2(side - 10.0, side - 10.0));
    let tex = texture(ui.ctx(), "pdn-vectorscope", generation, gain, || plane_image(&counts, gain));
    ui.painter().image(tex.id(), plot, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    let centre = plot.center();
    let radius = plot.width() / 2.0;
    // The ring, with a tick every 3 degrees (a longer one every 15), pointing in.
    ui.painter().circle_stroke(centre, radius, Stroke::new(1.2, GOLD));
    for step in 0..120 {
        let a = (step as f32 * 3.0).to_radians();
        let long = step % 5 == 0;
        let (dx, dy) = (a.cos(), -a.sin());
        let inner = radius * if long { 0.955 } else { 0.975 };
        line(ui, pos2(centre.x + dx * inner, centre.y + dy * inner), pos2(centre.x + dx * radius, centre.y + dy * radius), GOLD);
    }
    line(ui, pos2(plot.left(), centre.y), pos2(plot.right(), centre.y), GOLD);
    line(ui, pos2(centre.x, plot.top()), pos2(centre.x, plot.bottom()), GOLD);
    let a = SKIN_DEGREES.to_radians();
    line(ui, centre, pos2(centre.x + radius * a.cos(), centre.y - radius * a.sin()), GOLD);
    for (name, rgb) in TARGETS {
        let (cb, cr) = cb_cr(rgb.map(|v| v * 0.75));
        let p = plane_point(plot, cb, cr);
        let half = radius * 0.075;
        bracket(ui, p, half, Color32::from_rgb(205, 190, 125));
        ui.painter().text(pos2(p.x + half + 3.0, p.y + half * 0.5), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(10.0), GOLD_TEXT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn solid(w: usize, h: usize, c: [f32; 3]) -> Vec<[f32; 4]> {
        vec![[c[0], c[1], c[2], 1.0]; w * h]
    }

    #[test]
    fn cb_cr_of_the_primaries_are_the_bt709_ones() {
        let (cb, cr) = cb_cr([1.0, 0.0, 0.0]);
        assert!((cb + 0.11457).abs() < 1e-4 && (cr - 0.5).abs() < 1e-5);
        let (cb, cr) = cb_cr([0.0, 0.0, 1.0]);
        assert!((cb - 0.5).abs() < 1e-5 && (cr + 0.04585).abs() < 1e-4);
        let (cb, cr) = cb_cr([0.4, 0.4, 0.4]);
        assert!(cb.abs() < 1e-6 && cr.abs() < 1e-6, "grey is the centre");
    }

    #[test]
    fn a_flat_red_picture_fills_the_reds_top_row_and_the_others_bottom() {
        let c = count(&solid(32, 8, [1.0, 0.0, 0.0]), 32, 8);
        assert_eq!(c.samples, 256);
        // Every one of the 256 parade columns is covered (32 pixels spread over them), at one level.
        let level = |ch: usize, l: usize| (0..GRID).map(|col| c.parade[ch * GRID * GRID + l * GRID + col]).sum::<u32>();
        assert_eq!(level(0, GRID - 1), 256);
        assert_eq!(level(1, 0), 256);
        assert_eq!(level(2, 0), 256);
        // One cell of the plane, right of the centre in Cr and left in Cb.
        let filled: Vec<usize> = (0..GRID * GRID).filter(|&i| c.plane[i] > 0).collect();
        assert_eq!(filled.len(), 1);
        let (row, col) = (filled[0] / GRID, filled[0] % GRID);
        assert!(row > GRID / 2 && col < GRID / 2, "red: Cr up (high row), Cb a little negative");
    }

    #[test]
    fn grey_sits_in_the_middle_and_transparent_pixels_are_not_counted() {
        let mut px = solid(4, 4, [0.5, 0.5, 0.5]);
        px[0][3] = 0.0;
        let c = count(&px, 4, 4);
        assert_eq!(c.samples, 15);
        let i = c.plane.iter().position(|&n| n > 0).unwrap_or(0);
        assert!((i / GRID).abs_diff(GRID / 2) <= 1 && (i % GRID).abs_diff(GRID / 2) <= 1);
        assert_eq!(count(&[], 0, 0).samples, 0);
    }

    #[test]
    fn the_75_percent_targets_sit_inside_the_ring_where_resolve_puts_them() {
        // The red target is 0.385 out of a ring of 0.66: a bit over half the radius.
        let (cb, cr) = cb_cr([0.75, 0.0, 0.0]);
        let r = (cb * cb + cr * cr).sqrt() / REACH;
        assert!((0.55..0.62).contains(&r), "{r}");
        // Up and a little left, as in the picture.
        assert!(cr > 0.0 && cb < 0.0);
    }

    #[test]
    fn the_images_are_drawn_where_the_counts_are() {
        let c = count(&solid(16, 16, [1.0, 0.0, 0.0]), 16, 16);
        let p = parade_image(&c, 1.0);
        assert_eq!(p.size, [3 * GRID, GRID]);
        // Red's panel is lit along its top row, the green panel is dark there and lit at the bottom.
        assert!(p.pixels[0].r() > 120 && p.pixels[GRID].r() < 40);
        assert!(p.pixels[(GRID - 1) * 3 * GRID + GRID].g() > 120);
        let v = plane_image(&c, 1.0);
        assert_eq!(v.size, [GRID, GRID]);
        // A few pixels keep their colour; it is only a crowd that goes white.
        let few = plane_image(&count(&solid(3, 3, [1.0, 0.0, 0.0]), 3, 3), 1.0);
        assert!(few.pixels.iter().any(|p| p.r() > 120 && p.g() < p.r() / 2), "the trace keeps its colour");
        assert!(v.pixels.iter().any(|p| p.g() > 150), "a dense spot is white-hot");
        // A dense spot goes towards white; an empty picture is black.
        let flat = count(&solid(64, 64, [0.6, 0.4, 0.3]), 64, 64);
        assert!(plane_image(&flat, 1.0).pixels.iter().any(|p| p.g() > 150 && p.b() > 150));
        assert!(plane_image(&Counts::empty(), 1.0).pixels.iter().all(|p| *p == Color32::BLACK));
    }

    #[test]
    fn the_windows_draw_with_and_without_a_document_and_follow_edits() {
        use crate::Services;
        let ctx = Context::default();
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
        let frame = |app: &mut PhotocraftApp, f: fn(&mut PhotocraftApp, &mut Ui)| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(700.0, 500.0))), ..Default::default() };
            ctx.run_ui(raw, |ui| f(app, ui)).textures_delta.clear();
        };
        frame(&mut app, parade_body);
        frame(&mut app, vectorscope_body);
        app.run("file.new", json!({"width": 40, "height": 30, "background": "white"})).unwrap();
        frame(&mut app, parade_body);
        frame(&mut app, vectorscope_body);
        let first = shared(&ctx).lock().map(|c| c.generation).unwrap_or(0);
        assert!(first > 0, "the picture was counted");
        // A new state of the document is counted again (the wait is over at once for a tiny picture).
        app.run("grade.new", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        frame(&mut app, parade_body);
        assert!(shared(&ctx).lock().map(|c| c.generation).unwrap_or(0) > first);
    }
}
