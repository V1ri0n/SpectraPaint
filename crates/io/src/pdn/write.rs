//! Native PDN3 writer (Paint.NET's own format). The layout follows what Paint.NET 4.21 writes:
//! the `PDN3` magic and an XML header with a thumbnail, a .NET BinaryFormatter (MS-NRBF) object
//! graph describing the document and its bitmap layers, then each layer's BGRA pixels as
//! independent gzip chunks. Object ids are handed out in the same order as Paint.NET's
//! serializer so the graph is byte-for-byte what Paint.NET writes for the same content.
//!
//! Paint.NET layers are full-canvas bitmaps with a name, visibility, opacity and one of 14 blend
//! modes: groups, masks, effects, clipping and adjustment layers have no counterpart, so they are
//! baked into pixels (or left out) and reported as losses.

use std::collections::{HashMap, VecDeque};
use std::io::Write;

use photocraft_codecs::{self as codecs, ChannelLayout, Format, Image, SampleType as CSample};
use photocraft_color::{BlendMode, ColorMode, SampleType};
use photocraft_doc::{Document, Layer, LayerContent};
use photocraft_raster::Interrupt;

use super::{MAX_LAYERS, MAX_PIXEL_BYTES, MODES, OLD_MODES};
use crate::ora::Warnings;
use crate::{ExportResult, IoError};

type Result<T> = std::result::Result<T, IoError>;

/// The Paint.NET build whose file layout is written (and whose assemblies the graph names).
const VERSION: (i32, i32, i32, i32) = (4, 21, 6589, 7045);
const DATA_LIBRARY: &str = "PaintDotNet.Data, Version=4.21.6589.7045, Culture=neutral, PublicKeyToken=null";
const CORE_LIBRARY: &str = "PaintDotNet.Core, Version=4.21.6589.7045, Culture=neutral, PublicKeyToken=null";
const KVP: &str = "System.Collections.Generic.KeyValuePair`2[[System.String, mscorlib, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089],[System.String, mscorlib, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089]]";
/// Pixel bytes per gzip chunk (Paint.NET's own choice).
const CHUNK: usize = 262_144;
const THUMBNAIL_SIDE: u32 = 256;
const SOFTWARE: &str = "PhotoCraft";
/// The suffixes of the `$exif.tag<N>` keys Paint.NET gives the four EXIF entries it stores.
const EXIF_TAGS: [u32; 4] = [3, 7, 8, 9];

fn invalid(message: impl Into<String>) -> IoError {
    IoError::Pdn(message.into())
}

// ---------------------------------------------------------------------------------------------
// Planning: what a PDN file can hold of the document, and what it loses.

/// One output layer: an input layer and the PDN blend mode index it is saved with.
struct Item<'a> {
    layer: &'a Layer,
    mode: usize,
}

fn mode_index(mode: BlendMode) -> Option<usize> {
    let mode = if mode == BlendMode::PassThrough { BlendMode::Normal } else { mode };
    MODES.iter().position(|m| *m == mode)
}

/// The layers a PDN file will hold (adjustment layers cannot be) and the warnings for everything
/// the document has that it cannot keep. Errors when the document cannot be saved at all.
fn plan(doc: &Document) -> Result<(Vec<Item<'_>>, Warnings)> {
    if !matches!(doc.mode, ColorMode::Rgb | ColorMode::Grayscale) {
        return Err(IoError::Unsupported("Paint.NET holds RGB layers; convert the document to RGB or Grayscale first".into()));
    }
    let bytes = (doc.size.width as usize).checked_mul(doc.size.height as usize).and_then(|n| n.checked_mul(4));
    if doc.size.width == 0 || doc.size.height == 0 || doc.size.width > i32::MAX as u32 / 4 || doc.size.height > i32::MAX as u32 || bytes.is_none_or(|b| b > MAX_PIXEL_BYTES) {
        return Err(IoError::Unsupported("the canvas is too large for a Paint.NET file (1 GiB of pixels)".into()));
    }
    let mut w = Warnings::default();
    document_warnings(doc, &mut w);
    let mut items = Vec::new();
    for layer in &doc.layers {
        if matches!(layer.content, LayerContent::Adjustment(_)) {
            w.layer(&layer.name, "adjustment layers cannot be saved to Paint.NET and were left out (the merged image keeps their look)");
            continue;
        }
        let mode = mode_index(layer.blend).unwrap_or_else(|| {
            w.layer(&layer.name, &format!("the {} blend mode has no Paint.NET equivalent; it was saved as Normal", layer.blend.label()));
            0
        });
        layer_warnings(layer, doc, &mut w);
        items.push(Item { layer, mode });
    }
    if items.len() > MAX_LAYERS {
        return Err(IoError::Unsupported(format!("Paint.NET files hold at most {MAX_LAYERS} layers; merge some first")));
    }
    Ok((items, w))
}

fn document_warnings(doc: &Document, w: &mut Warnings) {
    if doc.depth != SampleType::U8 {
        w.note("Paint.NET holds 8 bits per channel; the layers were reduced to 8-bit");
    }
    if !doc.channels.is_empty() {
        w.note("alpha channels are not saved to Paint.NET");
    }
    if !doc.metadata.text.is_empty() || doc.metadata.xmp.is_some() || doc.metadata.exif.is_some() {
        w.note("metadata (XMP, EXIF, text) is not saved to Paint.NET");
    }
    let srgb = |bytes: &[u8]| photocraft_cms::Profile::parse(bytes).is_ok_and(|p| p.same_colors(photocraft_cms::Builtin::Srgb.profile()));
    if doc.icc_profile.as_deref().is_some_and(|b| !srgb(b)) {
        w.note("the colour profile is not saved to Paint.NET; it assumes sRGB");
    }
    if doc.mode == ColorMode::Grayscale {
        w.note("grayscale layers are saved as RGB");
    }
}

fn layer_warnings(layer: &Layer, doc: &Document, w: &mut Warnings) {
    let name = &layer.name;
    match &layer.content {
        LayerContent::Raster(s) => {
            if !s.content_bounds().is_empty() && s.content_bounds().intersect(&doc.bounds()) != s.content_bounds() {
                w.layer(name, "pixels outside the canvas were cropped");
            }
        }
        LayerContent::Group(_) => w.layer(name, "groups were merged into single layers"),
        other => w.layer(name, &format!("{} layers are saved to Paint.NET as pixels", other.kind_name().to_lowercase())),
    }
    if layer.mask.as_ref().is_some_and(|m| m.enabled) {
        w.layer(name, "layer masks were applied to the layer pixels");
    }
    if layer.clipped {
        w.layer(name, "Paint.NET has no clipping masks; these layers were saved unclipped");
    }
    if !layer.effects.items.is_empty() || layer.effects.psd_raw.is_some() {
        w.layer(name, "layer styles are not saved to Paint.NET");
    }
    if layer.vector_mask.as_ref().is_some_and(|m| m.enabled) {
        w.layer(name, "vector masks are not saved to Paint.NET");
    }
    if layer.fill_opacity < 1.0 {
        w.layer(name, "fill opacity was applied to the saved layer pixels");
    }
}

/// Everything saving `doc` to a Paint.NET file would lose (the warnings [`export`] gives),
/// without rendering anything. Empty when a `.pdn` holds the whole document, so it can be the
/// document's own file; otherwise a save to `.pdn` is a copy.
pub fn losses(doc: &Document) -> Vec<String> {
    match plan(doc) {
        Ok((_, w)) => w.finish(),
        Err(e) => vec![e.to_string()],
    }
}

// ---------------------------------------------------------------------------------------------
// Pixels

/// The layer as it shows on its own, straight BGRA over the whole canvas: its mask and fill
/// opacity applied, blending options (which the file stores separately) left out.
fn layer_bgra(doc: &Document, layer: &Layer) -> Vec<u8> {
    let canvas = doc.bounds();
    let mut px = match &layer.content {
        LayerContent::Raster(s) => photocraft_compose::surface_to_buffer(s, canvas).px,
        _ => {
            let mut solo = layer.clone();
            solo.visible = true;
            solo.opacity = 1.0;
            solo.fill_opacity = 1.0;
            solo.blend = BlendMode::Normal;
            solo.clipped = false;
            solo.mask = None;
            solo.effects = Default::default();
            photocraft_compose::render_layer(&solo, canvas).px
        }
    };
    if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
        let mut values = Vec::new();
        mask.values_into(canvas, &mut values);
        for (p, m) in px.iter_mut().zip(values) {
            p[3] *= m;
        }
    }
    let fill = layer.fill_opacity.clamp(0.0, 1.0);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut out = Vec::with_capacity(px.len() * 4);
    for p in &px {
        let a = byte(p[3] * fill);
        if a == 0 {
            out.extend_from_slice(&[0; 4]);
        } else {
            out.extend_from_slice(&[byte(p[2]), byte(p[1]), byte(p[0]), a]);
        }
    }
    out
}

/// One layer's pixel memory as the file stores it: format 0, the chunk size, then numbered,
/// individually gzipped chunks.
fn pixel_stream(bgra: &[u8], ctl: &Interrupt<'_>) -> Result<Vec<u8>> {
    let mut out = vec![0];
    out.extend_from_slice(&(CHUNK as u32).to_be_bytes());
    for (i, chunk) in bgra.chunks(CHUNK).enumerate() {
        ctl.check().map_err(|_| IoError::Cancelled)?;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(3));
        gz.write_all(chunk).map_err(|e| invalid(format!("compression failed: {e}")))?;
        let gz = gz.finish().map_err(|e| invalid(format!("compression failed: {e}")))?;
        out.extend_from_slice(&(i as u32).to_be_bytes());
        out.extend_from_slice(&(gz.len() as u32).to_be_bytes());
        out.extend_from_slice(&gz);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The object graph

/// Everything the graph records about one layer.
pub(super) struct LayerSpec {
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub mode: usize,
    pub background: bool,
}

/// Everything the graph records about the document.
pub(super) struct Spec {
    pub width: i32,
    pub height: i32,
    pub layers: Vec<LayerSpec>,
    /// The four `(key, value)` metadata strings.
    pub metadata: [(String, String); 4],
}

/// A member's declared type (MS-NRBF `BinaryTypeEnum` plus its additional information).
#[derive(Clone, Copy)]
enum Ty<'a> {
    Bool,
    Byte,
    Int32,
    Int64,
    Str,
    Class(&'a str, i32),
    System(&'a str),
    Objects,
}

enum Job {
    LayerList,
    Version,
    Metadata,
    Items,
    Layer(usize, i32),
    Props(usize, i32),
    Surface(usize, i32),
    LayerProps(usize, i32),
    BlendOp(usize, i32),
    Memory(usize, i32),
    EmptyMetadata(i32),
}

struct Graph<'a> {
    spec: &'a Spec,
    out: Vec<u8>,
    next: i32,
    queue: VecDeque<Job>,
    data_lib: i32,
    core_lib: i32,
    /// The first record of each class (the metadata later `ClassWithId` records point at).
    first_layer: i32,
    first_surface: i32,
    first_layer_props: i32,
    first_memory: i32,
    first_blend_mode: i32,
    shared_metadata: i32,
    props_by_mode: HashMap<usize, i32>,
    op_by_mode: HashMap<usize, i32>,
}

impl Graph<'_> {
    fn alloc(&mut self) -> i32 {
        let id = self.next;
        self.next += 1;
        id
    }
    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    fn lps(&mut self, s: &str) {
        let mut n = s.len();
        loop {
            let low = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                self.out.push(low);
                break;
            }
            self.out.push(low | 0x80);
        }
        self.out.extend_from_slice(s.as_bytes());
    }
    fn object_string(&mut self, id: i32, s: &str) {
        self.u8(6);
        self.i32(id);
        self.lps(s);
    }
    fn reference(&mut self, id: i32) {
        self.u8(9);
        self.i32(id);
    }
    /// A reference to an object not written yet: it gets the next id and is written later.
    fn new_ref(&mut self, job: impl FnOnce(i32) -> Job) {
        let id = self.alloc();
        self.reference(id);
        self.queue.push_back(job(id));
    }
    /// A reference to an object already registered. The serializer takes an id for it and
    /// drops it, which shows in every later id.
    fn known_ref(&mut self, id: i32) {
        self.alloc();
        self.reference(id);
    }

    /// `ClassWithMembersAndTypes` (with a library) or `SystemClassWithMembersAndTypes`.
    fn class(&mut self, id: i32, name: &str, members: &[(&str, Ty<'_>)], library: Option<i32>) {
        self.u8(if library.is_some() { 5 } else { 4 });
        self.i32(id);
        self.lps(name);
        self.i32(members.len() as i32);
        for (n, _) in members {
            self.lps(n);
        }
        for (_, t) in members {
            self.u8(match t {
                Ty::Bool | Ty::Byte | Ty::Int32 | Ty::Int64 => 0,
                Ty::Str => 1,
                Ty::System(_) => 3,
                Ty::Class(..) => 4,
                Ty::Objects => 5,
            });
        }
        for (_, t) in members {
            match *t {
                Ty::Bool => self.u8(1),
                Ty::Byte => self.u8(2),
                Ty::Int32 => self.u8(8),
                Ty::Int64 => self.u8(9),
                Ty::System(n) => self.lps(n),
                Ty::Class(n, lib) => {
                    self.lps(n);
                    self.i32(lib);
                }
                Ty::Str | Ty::Objects => {}
            }
        }
        if let Some(lib) = library {
            self.i32(lib);
        }
    }
    fn class_with_id(&mut self, id: i32, metadata: i32) {
        self.u8(1);
        self.i32(id);
        self.i32(metadata);
    }
    fn library(&mut self, id: i32, name: &str) {
        self.u8(12);
        self.i32(id);
        self.lps(name);
    }
    fn metadata_array(&mut self, id: i32, len: i32) {
        self.u8(7);
        self.i32(id);
        self.u8(0);
        self.i32(1);
        self.i32(len);
        self.u8(3);
        self.lps(KVP);
    }

    fn run(&mut self) {
        // SerializedStreamHeader: root object 1, no headers, version 1.0.
        self.u8(0);
        for v in [1, -1, 1, 0] {
            self.i32(v);
        }
        self.library(self.data_lib, DATA_LIBRARY);
        self.document();
        while let Some(job) = self.queue.pop_front() {
            match job {
                Job::LayerList => self.layer_list(),
                Job::Version => self.version(),
                Job::Metadata => self.metadata(),
                Job::Items => self.items(),
                Job::Layer(i, id) => self.layer(i, id),
                Job::Props(i, id) => self.props(i, id),
                Job::Surface(i, id) => self.surface(i, id),
                Job::LayerProps(i, id) => self.layer_props(i, id),
                Job::BlendOp(i, id) => self.blend_op(i, id),
                Job::Memory(i, id) => self.memory(i, id),
                Job::EmptyMetadata(id) => self.metadata_array(id, 0),
            }
        }
        self.u8(11);
    }

    fn document(&mut self) {
        let lib = self.data_lib;
        let kvp_array = format!("{KVP}[]");
        let members = [
            ("isDisposed", Ty::Bool),
            ("layers", Ty::Class("PaintDotNet.LayerList", lib)),
            ("width", Ty::Int32),
            ("height", Ty::Int32),
            ("savedWith", Ty::System("System.Version")),
            ("userMetadataItems", Ty::System(&kvp_array)),
        ];
        self.class(1, "PaintDotNet.Document", &members, Some(lib));
        self.u8(0);
        self.new_ref(|_| Job::LayerList);
        self.i32(self.spec.width);
        self.i32(self.spec.height);
        self.new_ref(|_| Job::Version);
        self.new_ref(|_| Job::Metadata);
    }

    fn layer_list(&mut self) {
        let lib = self.data_lib;
        let members = [("parent", Ty::Class("PaintDotNet.Document", lib)), ("ArrayList+_items", Ty::Objects), ("ArrayList+_size", Ty::Int32), ("ArrayList+_version", Ty::Int32)];
        self.class(3, "PaintDotNet.LayerList", &members, Some(lib));
        self.known_ref(1);
        self.new_ref(|_| Job::Items);
        let n = self.spec.layers.len() as i32;
        self.i32(n);
        self.i32(n);
    }

    fn version(&mut self) {
        let members = [("_Major", Ty::Int32), ("_Minor", Ty::Int32), ("_Build", Ty::Int32), ("_Revision", Ty::Int32)];
        self.class(4, "System.Version", &members, None);
        for v in [VERSION.0, VERSION.1, VERSION.2, VERSION.3] {
            self.i32(v);
        }
    }

    fn metadata(&mut self) {
        self.metadata_array(5, self.spec.metadata.len() as i32);
        let mut first = 0;
        let spec = self.spec;
        for (i, (key, value)) in spec.metadata.iter().enumerate() {
            let id = -self.alloc();
            if i == 0 {
                first = id;
                self.class(id, KVP, &[("key", Ty::Str), ("value", Ty::Str)], None);
            } else {
                self.class_with_id(id, first);
            }
            let (k, v) = (self.alloc(), self.alloc());
            self.object_string(k, key);
            self.object_string(v, value);
        }
    }

    /// The layer list's backing array: a power-of-two capacity (at least 4) padded with nulls.
    fn items(&mut self) {
        let n = self.spec.layers.len();
        let capacity = n.next_power_of_two().max(4);
        self.u8(16);
        self.i32(7);
        self.i32(capacity as i32);
        for i in 0..n {
            self.new_ref(|id| Job::Layer(i, id));
        }
        match capacity - n {
            0 => {}
            1 => self.u8(10),
            k if k <= 255 => {
                self.u8(13);
                self.u8(k as u8);
            }
            k => {
                self.u8(14);
                self.i32(k as i32);
            }
        }
    }

    fn layer(&mut self, i: usize, id: i32) {
        if i == 0 {
            self.core_lib = self.alloc();
            self.library(self.core_lib, CORE_LIBRARY);
            let (data, core) = (self.data_lib, self.core_lib);
            let members = [
                ("properties", Ty::Class("PaintDotNet.BitmapLayer+BitmapLayerProperties", data)),
                ("surface", Ty::Class("PaintDotNet.Surface", core)),
                ("Layer+isDisposed", Ty::Bool),
                ("Layer+width", Ty::Int32),
                ("Layer+height", Ty::Int32),
                ("Layer+properties", Ty::Class("PaintDotNet.Layer+LayerProperties", data)),
            ];
            self.class(id, "PaintDotNet.BitmapLayer", &members, Some(data));
            self.first_layer = id;
        } else {
            self.class_with_id(id, self.first_layer);
        }
        self.new_ref(|p| Job::Props(i, p));
        self.new_ref(|s| Job::Surface(i, s));
        self.u8(0);
        self.i32(self.spec.width);
        self.i32(self.spec.height);
        self.new_ref(|l| Job::LayerProps(i, l));
    }

    fn props(&mut self, i: usize, id: i32) {
        let mode = self.layer_mode(i);
        match self.props_by_mode.get(&mode) {
            Some(&meta) => self.class_with_id(id, meta),
            None => {
                let op = format!("PaintDotNet.UserBlendOps+{}BlendOp", OLD_MODES[mode]);
                let lib = self.data_lib;
                self.class(id, "PaintDotNet.BitmapLayer+BitmapLayerProperties", &[("blendOp", Ty::Class(&op, lib))], Some(lib));
                self.props_by_mode.insert(mode, id);
            }
        }
        self.new_ref(|o| Job::BlendOp(i, o));
    }

    fn surface(&mut self, i: usize, id: i32) {
        if i == 0 {
            let core = self.core_lib;
            let members = [("width", Ty::Int32), ("height", Ty::Int32), ("stride", Ty::Int32), ("scan0", Ty::Class("PaintDotNet.MemoryBlock", core))];
            self.class(id, "PaintDotNet.Surface", &members, Some(core));
            self.first_surface = id;
        } else {
            self.class_with_id(id, self.first_surface);
        }
        self.i32(self.spec.width);
        self.i32(self.spec.height);
        self.i32(self.spec.width * 4);
        self.new_ref(|m| Job::Memory(i, m));
    }

    fn layer_props(&mut self, i: usize, id: i32) {
        if i == 0 {
            let data = self.data_lib;
            let kvp_array = format!("{KVP}[]");
            let members = [
                ("name", Ty::Str),
                ("userMetadataItems", Ty::System(&kvp_array)),
                ("visible", Ty::Bool),
                ("isBackground", Ty::Bool),
                ("opacity", Ty::Byte),
                ("blendMode", Ty::Class("PaintDotNet.LayerBlendMode", data)),
            ];
            self.class(id, "PaintDotNet.Layer+LayerProperties", &members, Some(data));
            self.first_layer_props = id;
        } else {
            self.class_with_id(id, self.first_layer_props);
        }
        let spec = self.spec;
        let LayerSpec { name, visible, background, opacity, mode } = &spec.layers[i];
        let string = self.alloc();
        self.object_string(string, name);
        if i == 0 {
            self.shared_metadata = self.next;
            self.new_ref(Job::EmptyMetadata);
        } else {
            self.known_ref(self.shared_metadata);
        }
        self.u8(*visible as u8);
        self.u8(*background as u8);
        self.u8(*opacity);
        let blend = -self.alloc();
        if i == 0 {
            let data = self.data_lib;
            self.class(blend, "PaintDotNet.LayerBlendMode", &[("value__", Ty::Int32)], Some(data));
            self.first_blend_mode = blend;
        } else {
            self.class_with_id(blend, self.first_blend_mode);
        }
        self.i32(*mode as i32);
    }

    fn blend_op(&mut self, i: usize, id: i32) {
        let mode = self.layer_mode(i);
        match self.op_by_mode.get(&mode) {
            Some(&meta) => self.class_with_id(id, meta),
            None => {
                let name = format!("PaintDotNet.UserBlendOps+{}BlendOp", OLD_MODES[mode]);
                let lib = self.data_lib;
                self.class(id, &name, &[], Some(lib));
                self.op_by_mode.insert(mode, id);
            }
        }
    }

    fn memory(&mut self, i: usize, id: i32) {
        if i == 0 {
            let core = self.core_lib;
            self.class(id, "PaintDotNet.MemoryBlock", &[("length64", Ty::Int64), ("hasParent", Ty::Bool), ("deferred", Ty::Bool)], Some(core));
            self.first_memory = id;
        } else {
            self.class_with_id(id, self.first_memory);
        }
        let length = self.spec.width as i64 * 4 * self.spec.height as i64;
        self.out.extend_from_slice(&length.to_le_bytes());
        self.u8(0);
        self.u8(1);
    }

    fn layer_mode(&self, i: usize) -> usize {
        self.spec.layers.get(i).map_or(0, |l| l.mode)
    }
}

/// The serialized object graph (the bytes after the `00 01` marker) for `spec`.
pub(super) fn graph_bytes(spec: &Spec) -> Vec<u8> {
    let mut g = Graph {
        spec,
        out: Vec::new(),
        next: 3,
        queue: VecDeque::new(),
        data_lib: 2,
        core_lib: 0,
        first_layer: 0,
        first_surface: 0,
        first_layer_props: 0,
        first_memory: 0,
        first_blend_mode: 0,
        shared_metadata: 0,
        props_by_mode: HashMap::new(),
        op_by_mode: HashMap::new(),
    };
    g.run();
    g.out
}

// ---------------------------------------------------------------------------------------------
// The file

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
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

/// A `$exif.tag` metadata pair the way Paint.NET stores EXIF fields: an XML element holding the
/// TIFF tag id, type, byte length and base64 value.
fn exif_entry(slot: usize, id: u32, kind: u32, value: &[u8]) -> (String, String) {
    (format!("$exif.tag{}", EXIF_TAGS[slot]), format!("<exif id=\"{id}\" len=\"{}\" type=\"{kind}\" value=\"{}\" />", value.len(), base64(value)))
}

/// The four metadata entries Paint.NET writes: software, resolution unit (inches) and the two
/// resolutions as rationals.
pub(super) fn metadata(software: &str, dpi: f32) -> [(String, String); 4] {
    let mut software = software.as_bytes().to_vec();
    software.push(0);
    let dpi = (dpi.round().max(1.0) as u32).to_le_bytes();
    let rational = [dpi[0], dpi[1], dpi[2], dpi[3], 1, 0, 0, 0];
    [exif_entry(0, 305, 2, &software), exif_entry(1, 296, 3, &[2, 0]), exif_entry(2, 282, 5, &rational), exif_entry(3, 283, 5, &rational)]
}

fn thumbnail_png(doc: &Document) -> Result<String> {
    let t = photocraft_compose::thumbnail(doc, THUMBNAIL_SIDE);
    let image = Image::from_raw(t.width, t.height, ChannelLayout::Rgba, CSample::U8, t.pixels)?;
    let png = codecs::encode(&image, Format::Png, &codecs::EncodeOptions { embed_icc: false, ..Default::default() })?;
    Ok(base64(&png))
}

pub(crate) fn export(doc: &Document, ctl: &Interrupt<'_>) -> Result<ExportResult> {
    let (items, warnings) = plan(doc)?;
    let (width, height) = (doc.size.width as i32, doc.size.height as i32);
    // A document with nothing to save still needs the one layer a PDN must have.
    let blank;
    let items = if items.is_empty() {
        blank = Layer::raster("Background", photocraft_color::PixelFormat::RGBA8);
        vec![Item { layer: &blank, mode: 0 }]
    } else {
        items
    };
    let spec = Spec {
        width,
        height,
        layers: items
            .iter()
            .enumerate()
            .map(|(i, it)| LayerSpec { name: it.layer.name.clone(), visible: it.layer.visible, opacity: (it.layer.opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u8, mode: it.mode, background: i == 0 })
            .collect(),
        metadata: metadata(SOFTWARE, doc.resolution_dpi),
    };
    let header = format!(
        "<pdnImage width=\"{width}\" height=\"{height}\" layers=\"{}\" savedWithVersion=\"{}.{}.{}.{}\"><custom><thumb png=\"{}\" /></custom></pdnImage>",
        spec.layers.len(),
        VERSION.0,
        VERSION.1,
        VERSION.2,
        VERSION.3,
        thumbnail_png(doc)?
    );
    let header_len = u32::try_from(header.len()).ok().filter(|n| *n < 1 << 24).ok_or_else(|| invalid("header too large"))?;
    ctl.check().map_err(|_| IoError::Cancelled)?;
    ctl.progress(0.1);
    let mut bytes = b"PDN3".to_vec();
    bytes.extend_from_slice(&header_len.to_le_bytes()[..3]);
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&graph_bytes(&spec));
    for (i, item) in items.iter().enumerate() {
        ctl.check().map_err(|_| IoError::Cancelled)?;
        let bgra = layer_bgra(doc, item.layer);
        bytes.extend_from_slice(&pixel_stream(&bgra, ctl)?);
        ctl.progress(0.1 + 0.9 * (i + 1) as f32 / items.len() as f32);
    }
    Ok(ExportResult { bytes, warnings: warnings.finish() })
}
