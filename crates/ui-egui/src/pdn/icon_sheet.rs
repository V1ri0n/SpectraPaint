//! Redrawing the icons: a sheet of every icon of the Paint.NET layout, each with an empty square
//! beside it, to draw new ones in. The sheet is a plain PNG, so it can be painted in this program
//! or any other, saved, shared and loaded back; the program cuts the squares out and uses what is
//! drawn in place of the official icons. A square left empty keeps the official icon.
//!
//! Layout (fixed, so a sheet works in any editor and in any version that has at least its icons):
//! [`COLS`] pairs a row; a pair is the official icon on the left and the empty square on the right,
//! both [`CELL`] px. The icons run in the order of `icon_data::ICONS`; new icons are only ever
//! added at its end, so an older sheet keeps working. The squares are filled with [`KEY`], which is
//! what "nothing drawn" is: while the sheet is read, pixels of (nearly) that colour, and
//! transparent ones, become transparent, so an icon is drawn on that grey and comes out with no
//! background; edges that fade into the grey fade out.
//!
//! The icons in use are kept in the data folder (`icons/custom.png`, a sheet) and read at the next
//! start.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use egui::{Color32, Context, Id, Rect, TextureHandle, TextureOptions, Ui, pos2};
use photocraft_codecs::{self as codecs, ChannelLayout, Format, Image, SampleType};

use super::icon_data::ICONS;

/// Side of one icon square, in pixels.
pub const CELL: u32 = 64;
/// Pairs of squares in a row.
pub const COLS: usize = 4;
const MARGIN: u32 = 24;
const PAIR_GAP: u32 = 12;
const COL_GAP: u32 = 28;
const ROW_GAP: u32 = 24;
/// The colour of the sheet and of every square: what "nothing drawn" looks like.
pub const KEY: [u8; 3] = [40, 40, 44];
/// The frame round the empty squares.
const FRAME: [u8; 3] = [96, 150, 220];
/// How far from [`KEY`] a pixel is still the background, and how far it must be to be fully ink.
const KEY_NEAR: i32 = 8;
const KEY_FAR: i32 = 56;
/// Fewest ink pixels that make a square "drawn" (specks of noise do not).
const MIN_INK: usize = 8;

/// A redrawn icon: [`CELL`]² straight RGBA pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub rgba: Vec<u8>,
}

/// The sheet as pixels.
pub struct Sheet {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The icons the sheet has, in order.
pub fn names() -> Vec<&'static str> {
    ICONS.iter().map(|(n, _)| *n).collect()
}

fn rows_for(count: usize) -> u32 {
    count.div_ceil(COLS) as u32
}

/// The sheet's size for `count` icons.
pub fn sheet_size(count: usize) -> (u32, u32) {
    let rows = rows_for(count).max(1);
    let pair = CELL + PAIR_GAP + CELL;
    (2 * MARGIN + COLS as u32 * pair + (COLS as u32 - 1) * COL_GAP, 2 * MARGIN + rows * CELL + (rows - 1) * ROW_GAP)
}

/// Top-left pixel of the official (`blank` false) or the empty square of icon `i`.
fn origin(i: usize, blank: bool) -> (u32, u32) {
    let (col, row) = ((i % COLS) as u32, (i / COLS) as u32);
    let pair = CELL + PAIR_GAP + CELL;
    let x = MARGIN + col * (pair + COL_GAP) + if blank { CELL + PAIR_GAP } else { 0 };
    (x, MARGIN + row * (CELL + ROW_GAP))
}

fn put(rgba: &mut [u8], w: u32, x: u32, y: u32, c: [u8; 4]) {
    let i = ((y * w + x) * 4) as usize;
    if let Some(px) = rgba.get_mut(i..i + 4) {
        px.copy_from_slice(&c);
    }
}

/// An official icon rendered into a [`CELL`]² straight-RGBA square (transparent round it).
fn render_official(svg: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; (CELL * CELL * 4) as usize];
    let Ok(b) = photocraft_io::svg::rasterize(svg, 1.0) else { return out };
    // The icons are 24 px drawings: scale to fill the square.
    let side = b.rect.width().max(b.rect.height()).max(1) as f32;
    let Ok(b) = photocraft_io::svg::rasterize(svg, CELL as f32 / side) else { return out };
    let (w, h) = (b.rect.width(), b.rect.height());
    let (ox, oy) = ((CELL.saturating_sub(w)) / 2, (CELL.saturating_sub(h)) / 2);
    for y in 0..h.min(CELL) {
        for x in 0..w.min(CELL) {
            let p = b.px[(y * w + x) as usize];
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let i = (((y + oy) * CELL + (x + ox)) * 4) as usize;
            if let Some(d) = out.get_mut(i..i + 4) {
                d.copy_from_slice(&[q(p[0]), q(p[1]), q(p[2]), q(p[3])]);
            }
        }
    }
    out
}

/// The sheet. `custom` icons are laid in their squares (so the sheet of what is in use can be
/// saved and shared); the other squares are empty.
pub fn build(custom: &HashMap<String, Icon>) -> Sheet {
    let count = ICONS.len();
    let (w, h) = sheet_size(count);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..w * h {
        rgba.extend_from_slice(&[KEY[0], KEY[1], KEY[2], 255]);
    }
    for (i, (name, svg)) in ICONS.iter().enumerate() {
        // The official icon over the key colour.
        let (ox, oy) = origin(i, false);
        paste(&mut rgba, w, ox, oy, &render_official(svg), true);
        // The empty square, framed, with the redrawn icon in it when there is one.
        let (bx, by) = origin(i, true);
        frame(&mut rgba, w, bx, by);
        if let Some(icon) = custom.get(*name) {
            paste(&mut rgba, w, bx, by, &icon.rgba, true);
        }
    }
    Sheet { width: w, height: h, rgba }
}

/// Lays a [`CELL`]² RGBA square at `(x0, y0)`, over what is there when `over`.
fn paste(rgba: &mut [u8], w: u32, x0: u32, y0: u32, cell: &[u8], over: bool) {
    for y in 0..CELL {
        for x in 0..CELL {
            let i = ((y * CELL + x) * 4) as usize;
            let Some(s) = cell.get(i..i + 4) else { return };
            let a = u32::from(s[3]);
            if a == 0 && over {
                continue;
            }
            let at = (((y0 + y) * w + (x0 + x)) * 4) as usize;
            let Some(d) = rgba.get(at..at + 4) else { continue };
            let blend = |s: u8, d: u8| ((u32::from(s) * a + u32::from(d) * (255 - a) + 127) / 255) as u8;
            let out = [blend(s[0], d[0]), blend(s[1], d[1]), blend(s[2], d[2]), 255];
            put(rgba, w, x0 + x, y0 + y, out);
        }
    }
}

/// A 2 px frame just outside the square at `(x0, y0)`.
fn frame(rgba: &mut [u8], w: u32, x0: u32, y0: u32) {
    let c = [FRAME[0], FRAME[1], FRAME[2], 255];
    for t in 1..=2u32 {
        for k in 0..CELL + 2 * t {
            let (x, y) = (x0 + k - t, y0 - t);
            put(rgba, w, x, y, c);
            put(rgba, w, x, y0 + CELL + t - 1, c);
            put(rgba, w, x0 - t, y0 + k - t, c);
            put(rgba, w, x0 + CELL + t - 1, y0 + k - t, c);
        }
    }
}

/// How much of a pixel is ink, 0..=255: none at the key colour, all well away from it.
fn ink(px: &[u8]) -> u8 {
    if px[3] == 0 {
        return 0;
    }
    let d = (0..3).map(|i| (i32::from(px[i]) - i32::from(KEY[i])).abs()).max().unwrap_or(0);
    let k = if d <= KEY_NEAR { 0 } else if d >= KEY_FAR { 255 } else { (d - KEY_NEAR) * 255 / (KEY_FAR - KEY_NEAR) };
    ((k * i32::from(px[3])) / 255) as u8
}

/// The redrawn icons in a sheet's pixels: the ones whose empty square has something drawn in it.
/// A sheet cut short (an older one with fewer icons) gives the icons it has; one of another width
/// is no sheet of ours.
pub fn extract(width: u32, height: u32, rgba: &[u8]) -> Result<HashMap<String, Icon>, String> {
    let (w, _) = sheet_size(ICONS.len());
    if width != w {
        return Err(format!("this is not an icon sheet: its width is {width} px, a sheet is {w} px wide"));
    }
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err("the image's pixels do not match its size".into());
    }
    let mut out = HashMap::new();
    for (i, (name, _)) in ICONS.iter().enumerate() {
        let (bx, by) = origin(i, true);
        if by + CELL > height {
            break;
        }
        let mut cell = vec![0u8; (CELL * CELL * 4) as usize];
        let mut inked = 0usize;
        for y in 0..CELL {
            for x in 0..CELL {
                let at = (((by + y) * width + (bx + x)) * 4) as usize;
                let px = &rgba[at..at + 4];
                let a = ink(px);
                if a > 0 {
                    inked += 1;
                    let o = ((y * CELL + x) * 4) as usize;
                    cell[o..o + 3].copy_from_slice(&px[..3]);
                    cell[o + 3] = a;
                }
            }
        }
        if inked >= MIN_INK {
            out.insert((*name).to_string(), Icon { rgba: cell });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// PNG

/// A sheet as a PNG file.
pub fn encode_png(sheet: &Sheet) -> Result<Vec<u8>, String> {
    let image = Image::from_raw(sheet.width, sheet.height, ChannelLayout::Rgba, SampleType::U8, sheet.rgba.clone()).map_err(|e| e.to_string())?;
    codecs::encode(&image, Format::Png, &codecs::EncodeOptions { embed_icc: false, ..Default::default() }).map_err(|e| e.to_string())
}

/// The redrawn icons of a PNG (or any image file) of a sheet.
pub fn read_png(bytes: &[u8]) -> Result<HashMap<String, Icon>, String> {
    let img = codecs::decode(bytes).map_err(|e| e.to_string())?.convert(ChannelLayout::Rgba, SampleType::U8);
    extract(img.width(), img.height(), img.data())
}

// ---------------------------------------------------------------------------------------------
// The icons in use

struct InUse {
    icons: HashMap<String, Arc<Icon>>,
    /// Changes whenever the icons do, so cached textures are renewed.
    generation: u64,
    loaded: bool,
}

static IN_USE: RwLock<Option<InUse>> = RwLock::new(None);

fn with_state<R>(f: impl FnOnce(&mut InUse) -> R) -> Option<R> {
    let mut g = IN_USE.write().ok()?;
    Some(f(g.get_or_insert_with(|| InUse { icons: HashMap::new(), generation: 0, loaded: false })))
}

/// The redrawn icon `name`, with the generation it belongs to.
pub fn custom(name: &str) -> Option<(Arc<Icon>, u64)> {
    let g = IN_USE.read().ok()?;
    let st = g.as_ref()?;
    st.icons.get(name).map(|i| (i.clone(), st.generation))
}

/// How many icons are redrawn.
pub fn count() -> usize {
    IN_USE.read().ok().and_then(|g| g.as_ref().map(|s| s.icons.len())).unwrap_or(0)
}

/// Uses `icons` from now on (replacing the ones in use) and keeps them for the next start.
pub fn use_icons(icons: HashMap<String, Icon>) -> Result<(), String> {
    with_state(|st| {
        st.icons = icons.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        st.generation += 1;
        st.loaded = true;
    });
    save_in_use()
}

/// The sheet of the icons in use.
pub fn sheet_in_use() -> Sheet {
    let map: HashMap<String, Icon> = IN_USE
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.icons.iter().map(|(k, v)| (k.clone(), (**v).clone())).collect()))
        .unwrap_or_default();
    build(&map)
}

/// Where the kept icons go instead of the data folder (tests).
static DIR_OVERRIDE: RwLock<Option<std::path::PathBuf>> = RwLock::new(None);

fn file() -> Option<std::path::PathBuf> {
    if let Some(d) = DIR_OVERRIDE.read().ok().and_then(|g| g.clone()) {
        return Some(d.join("icons").join("custom.png"));
    }
    let dir = if let Some(d) = std::env::var_os("PHOTOCRAFT_CONFIG_DIR").filter(|d| !d.is_empty()) {
        std::path::PathBuf::from(d)
    } else {
        let exe = std::env::current_exe().ok()?.parent()?.to_path_buf();
        let portable = exe.join("PhotoCraftData");
        if portable.is_dir() { portable } else { std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("PhotoCraft") }
    };
    Some(dir.join("icons").join("custom.png"))
}

/// Keeps the icons in use (no file at all when none is redrawn).
fn save_in_use() -> Result<(), String> {
    let Some(path) = file() else { return Ok(()) };
    if count() == 0 {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let png = encode_png(&sheet_in_use())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads the kept icons, once per run.
pub fn load_kept() {
    if with_state(|st| std::mem::replace(&mut st.loaded, true)).unwrap_or(true) {
        return;
    }
    let Some(path) = file() else { return };
    if let Ok(bytes) = std::fs::read(&path)
        && let Ok(icons) = read_png(&bytes)
    {
        with_state(|st| {
            st.icons = icons.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
            st.generation += 1;
        });
    }
}

/// Paints a redrawn icon centred in `rect`; false when `name` is not redrawn.
pub fn paint(ui: &Ui, rect: Rect, name: &str, size: f32, tint: Color32) -> bool {
    let Some((icon, generation)) = custom(name) else { return false };
    let tex = texture(ui.ctx(), name, generation, &icon);
    let r = Rect::from_center_size(rect.center(), egui::Vec2::splat(size));
    ui.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
    true
}

fn texture(ctx: &Context, name: &str, generation: u64, icon: &Icon) -> TextureHandle {
    let id = Id::new(("pdn-custom-icon", name, generation));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([CELL as usize, CELL as usize], &icon.rgba);
    // Mipmapped, so a 64 px drawing stays clean at the 24-30 px the buttons show it at.
    let options = TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..TextureOptions::LINEAR };
    let t = ctx.load_texture(format!("pdn-custom-icon-{name}-{generation}"), image, options);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_px(sheet: &Sheet, i: usize, blank: bool, x: u32, y: u32) -> [u8; 4] {
        let (ox, oy) = origin(i, blank);
        let at = (((oy + y) * sheet.width + ox + x) * 4) as usize;
        [sheet.rgba[at], sheet.rgba[at + 1], sheet.rgba[at + 2], sheet.rgba[at + 3]]
    }

    fn draw_square(sheet: &mut Sheet, i: usize, colour: [u8; 4]) {
        let (ox, oy) = origin(i, true);
        for y in 20..44 {
            for x in 20..44 {
                put(&mut sheet.rgba, sheet.width, ox + x, oy + y, colour);
            }
        }
    }

    #[test]
    fn the_sheet_has_a_pair_for_every_icon_and_the_squares_do_not_overlap() {
        let n = ICONS.len();
        assert!(n > 40);
        let (w, h) = sheet_size(n);
        let mut boxes = Vec::new();
        for i in 0..n {
            for blank in [false, true] {
                let (x, y) = origin(i, blank);
                assert!(x >= MARGIN && y >= MARGIN && x + CELL + MARGIN <= w && y + CELL + MARGIN <= h, "icon {i} fits");
                boxes.push((x, y));
            }
        }
        boxes.sort_unstable();
        boxes.dedup();
        assert_eq!(boxes.len(), n * 2, "every square has its own place");
        // The official icons are drawn and the squares are empty.
        let sheet = build(&HashMap::new());
        assert_eq!((sheet.width, sheet.height), (w, h));
        assert!((0..CELL).any(|x| (0..CELL).any(|y| cell_px(&sheet, 0, false, x, y)[..3] != KEY)), "the official icon shows");
        assert!((0..CELL).all(|x| (0..CELL).all(|y| cell_px(&sheet, 0, true, x, y)[..3] == KEY)), "the square is empty");
    }

    #[test]
    fn an_untouched_sheet_redraws_nothing() {
        let sheet = build(&HashMap::new());
        assert!(extract(sheet.width, sheet.height, &sheet.rgba).unwrap().is_empty());
    }

    #[test]
    fn what_is_drawn_in_a_square_is_cut_out_with_no_background() {
        let mut sheet = build(&HashMap::new());
        draw_square(&mut sheet, 3, [220, 30, 30, 255]);
        let found = extract(sheet.width, sheet.height, &sheet.rgba).unwrap();
        assert_eq!(found.len(), 1);
        let icon = &found[names()[3]];
        let at = |x: u32, y: u32| {
            let i = ((y * CELL + x) * 4) as usize;
            [icon.rgba[i], icon.rgba[i + 1], icon.rgba[i + 2], icon.rgba[i + 3]]
        };
        assert_eq!(at(30, 30), [220, 30, 30, 255], "ink keeps its colour");
        assert_eq!(at(2, 2)[3], 0, "the grey around it is gone");
        // Drawing in the official icon's square, or specks, changes nothing.
        let mut other = build(&HashMap::new());
        let (ox, oy) = origin(5, false);
        for y in 0..10 {
            put(&mut other.rgba, other.width, ox, oy + y, [255, 0, 0, 255]);
        }
        let (bx, by) = origin(6, true);
        put(&mut other.rgba, other.width, bx + 10, by + 10, [255, 255, 255, 255]);
        assert!(extract(other.width, other.height, &other.rgba).unwrap().is_empty());
    }

    #[test]
    fn transparent_pixels_and_faded_edges_are_not_ink() {
        let mut sheet = build(&HashMap::new());
        draw_square(&mut sheet, 0, [0, 0, 0, 0]);
        assert!(extract(sheet.width, sheet.height, &sheet.rgba).unwrap().is_empty(), "erased to transparent");
        assert_eq!(ink(&[KEY[0], KEY[1], KEY[2], 255]), 0);
        assert_eq!(ink(&[KEY[0] + 4, KEY[1], KEY[2], 255]), 0);
        assert_eq!(ink(&[255, 255, 255, 255]), 255);
        let half = ink(&[KEY[0] + 32, KEY[1], KEY[2], 255]);
        assert!(half > 0 && half < 255, "{half}");
        assert_eq!(ink(&[255, 255, 255, 0]), 0);
    }

    #[test]
    fn redrawn_icons_go_into_a_saved_sheet_and_come_back() {
        let mut sheet = build(&HashMap::new());
        draw_square(&mut sheet, 7, [30, 200, 60, 255]);
        let mut icons = extract(sheet.width, sheet.height, &sheet.rgba).unwrap();
        // Through a PNG file, as a shared pack travels.
        let again = build(&icons);
        let png = encode_png(&again).unwrap();
        let back = read_png(&png).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[names()[7]], icons.remove(names()[7]).unwrap());
    }

    #[test]
    fn a_sheet_of_another_size_or_an_older_sheet() {
        let sheet = build(&HashMap::new());
        assert!(extract(sheet.width + 1, sheet.height, &sheet.rgba).is_err());
        assert!(extract(sheet.width, sheet.height, &sheet.rgba[4..]).is_err());
        // An older sheet with fewer rows still works for the icons it has.
        let mut sheet = build(&HashMap::new());
        draw_square(&mut sheet, 1, [200, 200, 0, 255]);
        let (_, h_short) = sheet_size(ICONS.len() - COLS * 3);
        let cut = &sheet.rgba[..(sheet.width * h_short * 4) as usize];
        let found = extract(sheet.width, h_short, cut).unwrap();
        assert!(found.contains_key(names()[1]));
        assert!(read_png(b"not a png").is_err());
    }

    /// Writes the empty sheet to the path in `SPECTRA_SHEET_OUT` (for handing one out without
    /// starting the program): `SPECTRA_SHEET_OUT=sheet.png cargo test -p photocraft-ui-egui --lib
    /// write_the_sheet -- --ignored`.
    #[test]
    #[ignore]
    fn write_the_sheet() {
        let path = std::env::var("SPECTRA_SHEET_OUT").expect("set SPECTRA_SHEET_OUT");
        std::fs::write(path, encode_png(&build(&HashMap::new())).unwrap()).unwrap();
    }

    #[test]
    fn icons_in_use_are_found_by_name() {
        // Global state: this is the one test that touches it.
        if let Ok(mut d) = DIR_OVERRIDE.write() {
            *d = Some(std::env::temp_dir().join("spectra-icon-test"));
        }
        let mut sheet = build(&HashMap::new());
        draw_square(&mut sheet, 2, [10, 20, 250, 255]);
        let icons = extract(sheet.width, sheet.height, &sheet.rgba).unwrap();
        assert!(custom(names()[2]).is_none() || count() > 0);
        use_icons(icons).unwrap();
        assert_eq!(count(), 1);
        let (icon, generation) = custom(names()[2]).unwrap();
        assert_eq!(icon.rgba.len(), (CELL * CELL * 4) as usize);
        assert!(custom(names()[3]).is_none());
        // Using other icons renews the generation; none at all clears the kept file.
        use_icons(HashMap::new()).unwrap();
        assert_eq!(count(), 0);
        assert!(custom(names()[2]).is_none());
        assert!(IN_USE.read().unwrap().as_ref().unwrap().generation > generation);
        assert!(file().is_some_and(|f| !f.exists()));
    }
}
