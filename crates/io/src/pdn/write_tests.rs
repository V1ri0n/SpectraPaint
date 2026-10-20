use super::write::{graph_bytes, metadata, LayerSpec, Spec};
use super::*;

/// Offsets of the first differing byte, with a window of both sides, instead of a wall of bytes.
fn assert_same_bytes(actual: &[u8], expected: &[u8], what: &str) {
    let at = actual.iter().zip(expected).position(|(a, b)| a != b).unwrap_or(actual.len().min(expected.len()));
    if actual.len() != expected.len() || at < actual.len().min(expected.len()) {
        let window = |b: &[u8]| b.get(at.saturating_sub(24)..(at + 24).min(b.len())).map(|w| w.to_vec()).unwrap_or_default();
        panic!("{what}: lengths {} vs {}, first difference at {at}\nactual   {:?}\nexpected {:?}", actual.len(), expected.len(), window(actual), window(expected));
    }
}

/// The document description recorded in a graph Paint.NET wrote.
fn spec_of(graph_bytes: &[u8]) -> Spec {
    let mut reader = Reader { bytes: graph_bytes, pos: 0 };
    let graph = nrbf::parse(&mut reader, &Interrupt::default()).unwrap();
    assert_eq!(reader.pos, graph_bytes.len(), "the fixture is exactly one graph");
    let root = &graph.root;
    let list = graph.field(root, "layers").unwrap();
    let items = graph.array(graph.field(list, "ArrayList+_items").unwrap()).unwrap();
    let size = count(&graph, list, "ArrayList+_size").unwrap();
    let layers = items[..size]
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            let props = graph.field(layer, "Layer+properties").unwrap();
            LayerSpec {
                name: graph.string(graph.field(props, "name").unwrap()).unwrap().to_string(),
                visible: graph.boolean(graph.field(props, "visible").unwrap()).unwrap(),
                opacity: count(&graph, props, "opacity").unwrap() as u8,
                mode: MODES.iter().position(|m| *m == blend(&graph, layer, props).unwrap()).unwrap(),
                background: i == 0 && graph.boolean(graph.field(props, "isBackground").unwrap()).unwrap(),
            }
        })
        .collect();
    let entries = graph.array(graph.field(root, "userMetadataItems").unwrap()).unwrap();
    let pair = |i: usize| {
        let e = &entries[i];
        (graph.string(graph.field(e, "key").unwrap()).unwrap().to_string(), graph.string(graph.field(e, "value").unwrap()).unwrap().to_string())
    };
    Spec { width: count(&graph, root, "width").unwrap() as i32, height: count(&graph, root, "height").unwrap() as i32, layers, metadata: [pair(0), pair(1), pair(2), pair(3)] }
}

#[test]
fn the_object_graph_is_what_paint_net_writes_byte_for_byte() {
    // One layer; two layers (a shared empty metadata array, a repeated class); three non-Normal
    // layers with distinct blend ops; and fourteen layers, one per blend mode, that need a
    // 16-slot list with two trailing nulls.
    for (name, bytes) in [
        ("Untitled", &include_bytes!("fixtures/Untitled.nrbf")[..]),
        ("Untitled2", &include_bytes!("fixtures/Untitled2.nrbf")[..]),
        ("Untitled3", &include_bytes!("fixtures/Untitled3.nrbf")[..]),
        ("FlattenBlendTest", &include_bytes!("fixtures/FlattenBlendTest.nrbf")[..]),
    ] {
        let spec = spec_of(bytes);
        assert_same_bytes(&graph_bytes(&spec), bytes, name);
    }
}

#[test]
fn the_writers_own_metadata_has_the_four_exif_entries() {
    let m = metadata("PhotoCraft", 300.0);
    assert_eq!(m[0].1, "<exif id=\"305\" len=\"11\" type=\"2\" value=\"UGhvdG9DcmFmdAA=\" />");
    assert_eq!(m[1].1, "<exif id=\"296\" len=\"2\" type=\"3\" value=\"AgA=\" />");
    // 300/1 as two little-endian words.
    assert_eq!(m[2].1, "<exif id=\"282\" len=\"8\" type=\"5\" value=\"LAEAAAEAAAA=\" />");
    assert_eq!(m[2].1.replace("282", "283"), m[3].1);
    // The sample's 96 dpi.
    assert!(metadata("paint.net 4.0.21", 96.0)[0].1.contains("cGFpbnQubmV0IDQuMC4yMQA="));
    assert!(metadata("paint.net 4.0.21", 96.0)[2].1.contains("YAAAAAEAAAA="));
}

fn px(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    [r, g, b, a]
}

/// A `w`×`h` raster layer whose pixels are a function of position and `seed`.
fn layer(name: &str, w: u32, h: u32, seed: u8) -> Layer {
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            data.extend(px((x as u8).wrapping_mul(7).wrapping_add(seed), (y as u8).wrapping_mul(3), seed.wrapping_mul(5), if (x + y) % 5 == 0 { 0 } else { 40 + ((x * 3 + y) % 200) as u8 }));
        }
    }
    let mut surface = Surface::new(PixelFormat::RGBA8);
    surface.write_interleaved(Rect::from_xywh(0, 0, w, h), &data);
    let mut l = Layer::raster(name, PixelFormat::RGBA8);
    l.content = LayerContent::Raster(surface);
    l
}

fn document(w: u32, h: u32, layers: Vec<Layer>) -> Document {
    let mut doc = Document::new("Doc", Size::new(w, h), ColorMode::Rgb, SampleType::U8);
    doc.layers = layers;
    doc
}

fn bytes_of(doc: &Document) -> crate::ExportResult {
    crate::export(doc, "out.pdn", &Default::default()).unwrap()
}

fn rgba8(l: &Layer) -> Vec<u8> {
    let s = l.surface().unwrap();
    let mut out = Vec::new();
    for y in 0..s.content_bounds().y1.max(0) {
        for x in 0..s.content_bounds().x1.max(0) {
            let p: Vec<u8> = s.pixel(x, y).iter().map(|v| (v * 255.0).round() as u8).collect();
            // Fully transparent pixels have no colour to compare (the file stores zeros).
            out.extend(if p[3] == 0 { vec![0; 4] } else { p });
        }
    }
    out
}

#[test]
fn a_document_survives_a_save_and_reopen() {
    let mut layers = vec![layer("Background", 13, 9, 1), layer("Ünï côde ✓", 13, 9, 2), layer("Hidden", 13, 9, 3), layer("Faded", 13, 9, 4)];
    layers[1].blend = BlendMode::Multiply;
    layers[2].visible = false;
    layers[2].blend = BlendMode::Xor;
    layers[3].opacity = 0.5;
    layers[3].blend = BlendMode::PaintNetColorDodge;
    let doc = document(13, 9, layers);
    let saved = bytes_of(&doc);
    assert!(saved.warnings.is_empty(), "{:?}", saved.warnings);
    assert!(saved.bytes.starts_with(b"PDN3"));
    let back = crate::import("out.pdn", &saved.bytes).unwrap();
    assert!(!back.source_read_only, "a native writer makes .pdn a real save target");
    let back = back.document;
    assert_eq!((back.size.width, back.size.height), (13, 9));
    assert_eq!(back.layers.len(), 4);
    for (a, b) in doc.layers.iter().zip(&back.layers) {
        assert_eq!((&a.name, a.visible, a.blend), (&b.name, b.visible, b.blend));
        assert!((a.opacity - b.opacity).abs() < 1.0 / 255.0);
        assert_eq!(rgba8(a), rgba8(b), "{}", a.name);
    }
    // And a second round trip changes nothing.
    let again = crate::import("out.pdn", &bytes_of(&back).bytes).unwrap().document;
    for (a, b) in back.layers.iter().zip(&again.layers) {
        assert_eq!((&a.name, a.visible, a.blend, a.opacity, rgba8(a)), (&b.name, b.visible, b.blend, b.opacity, rgba8(b)));
    }
}

#[test]
fn the_header_describes_the_document_and_holds_a_thumbnail() {
    let saved = bytes_of(&document(40, 20, vec![layer("A", 40, 20, 1), layer("B", 40, 20, 2)]));
    let n = u32::from_le_bytes([saved.bytes[4], saved.bytes[5], saved.bytes[6], 0]) as usize;
    let header = std::str::from_utf8(&saved.bytes[7..7 + n]).unwrap();
    assert!(header.starts_with("<pdnImage width=\"40\" height=\"20\" layers=\"2\" savedWithVersion=\"4.21."), "{header}");
    assert!(header.contains("<custom><thumb png=\"iVBORw0KGgo"), "a base64 PNG");
    assert!(header.ends_with("\" /></custom></pdnImage>"));
    assert_eq!(&saved.bytes[7 + n..9 + n], [0, 1]);
}

#[test]
fn layer_counts_around_the_list_capacities_all_round_trip() {
    // 1..=4 share the 4-slot list (3 gives a lone null, 4 none); 5 and 9 grow it to 8 and 16;
    // 17 needs 32 slots, 15 of them null.
    for n in [1, 2, 3, 4, 5, 8, 9, 17] {
        let layers = (0..n).map(|i| layer(&format!("L{i}"), 6, 5, i as u8)).collect();
        let doc = document(6, 5, layers);
        let back = crate::import("n.pdn", &bytes_of(&doc).bytes).unwrap().document;
        assert_eq!(back.layers.len(), n, "{n} layers");
        assert_eq!(rgba8(&back.layers[n - 1]), rgba8(&doc.layers[n - 1]));
        assert_eq!(back.layers[n - 1].name, format!("L{}", n - 1));
    }
}

#[test]
fn a_canvas_bigger_than_one_chunk_is_split_and_restored() {
    // 300 × 300 × 4 = 360 000 bytes: two 262 144-byte chunks.
    let doc = document(300, 300, vec![layer("Big", 300, 300, 9)]);
    let back = crate::import("big.pdn", &bytes_of(&doc).bytes).unwrap().document;
    assert_eq!(rgba8(&back.layers[0]), rgba8(&doc.layers[0]));
}

#[test]
fn what_a_pdn_cannot_hold_is_baked_or_left_out_and_reported() {
    let mut masked = layer("Masked", 8, 8, 1);
    masked.fill_opacity = 0.5;
    let mut odd_blend = layer("Soft", 8, 8, 2);
    odd_blend.blend = BlendMode::SoftLight;
    let mut clipped = layer("Clipped", 8, 8, 3);
    clipped.clipped = true;
    let group = Layer::group("Group", vec![layer("Inner", 8, 8, 4)]);
    let adjustment = {
        Layer::new("Contrast", LayerContent::Adjustment(photocraft_doc::Adjustment::BrightnessContrast { brightness: 10.0, contrast: 5.0, legacy: false }))
    };
    let doc = document(8, 8, vec![layer("Base", 8, 8, 0), masked, odd_blend, clipped, group, adjustment]);
    let saved = bytes_of(&doc);
    let joined = saved.warnings.join("\n");
    for expected in ["fill opacity", "blend mode has no Paint.NET equivalent", "clipping", "groups were merged", "adjustment layers"] {
        assert!(joined.contains(expected), "{expected} in {joined}");
    }
    let back = crate::import("x.pdn", &saved.bytes).unwrap().document;
    assert_eq!(back.layers.len(), 5, "the adjustment layer is left out");
    assert_eq!(back.layers[2].blend, BlendMode::Normal);
    // Fill opacity 0.5 halves the baked alpha.
    let alpha = |l: &Layer| l.surface().unwrap().pixel(1, 0)[3];
    assert!((alpha(&back.layers[1]) - alpha(&doc.layers[1]) * 0.5).abs() < 0.01);
    assert_eq!(losses(&doc), saved.warnings, "the dry run reports the same losses");
}

#[test]
fn a_plain_document_has_no_losses() {
    let doc = document(5, 5, vec![layer("A", 5, 5, 1), layer("B", 5, 5, 2)]);
    assert!(losses(&doc).is_empty());
}

#[test]
fn an_empty_document_still_writes_the_one_layer_a_pdn_needs() {
    let back = crate::import("e.pdn", &bytes_of(&document(7, 3, Vec::new())).bytes).unwrap().document;
    assert_eq!(back.layers.len(), 1);
    assert_eq!(back.layers[0].name, "Background");
}

#[test]
fn colour_modes_a_pdn_cannot_hold_are_refused() {
    let mut doc = document(4, 4, vec![layer("A", 4, 4, 1)]);
    doc.mode = ColorMode::Cmyk;
    assert!(crate::export(&doc, "c.pdn", &Default::default()).is_err());
    assert_eq!(losses(&doc).len(), 1);
}

#[test]
fn save_can_be_cancelled() {
    let doc = document(300, 300, vec![layer("A", 300, 300, 1)]);
    let cancel = || true;
    let result = super::write::export(&doc, &Interrupt::cancel_only(&cancel));
    assert!(matches!(result, Err(IoError::Cancelled)));
}
