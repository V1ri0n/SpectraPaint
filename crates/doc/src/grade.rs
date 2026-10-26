//! Node-based colour grading data: the graph an adjustment layer of kind
//! [`Adjustment::NodeGrade`](crate::Adjustment) holds. Pure data; the maths lives in
//! `photocraft_compose::grade`.
//!
//! The model follows the way DaVinci Resolve's Color page is worked (serial and parallel nodes,
//! mixers, a corrector with wheels, curves, a qualifier and windows), but every formula is this
//! program's own: Resolve's are not public, so a grade here looks like the same kind of move, not
//! the same pixels.
//!
//! A graph is a DAG of [`GradeNode`]s. A node's `inputs` name the nodes it reads (`None`: the
//! layer's source image); `output` names the node whose result the layer gives. Every value is
//! stored so that its `Default` is "no change".

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::adjust::CurvePoint;
use photocraft_color::BlendMode;

/// Most nodes a graph may hold.
pub const MAX_NODES: usize = 64;
/// Most inputs a mixer may read.
pub const MAX_INPUTS: usize = 8;
/// Most points on one curve.
pub const MAX_CURVE_POINTS: usize = 64;
/// Most windows on one node.
pub const MAX_WINDOWS: usize = 8;
/// Largest 3D LUT a LUT node holds (a side of this many entries).
pub const MAX_LUT_SIZE: u32 = 65;

pub type NodeId = u32;

/// A colour wheel's puck: how far the colour is pushed towards red, green and blue, and the
/// master slider under the wheel. All in -1..=1; zero is neutral.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Wheel {
    pub rgb: [f32; 3],
    pub master: f32,
}

impl Wheel {
    /// The push on each channel: its own puck value plus the master.
    pub fn channels(&self) -> [f32; 3] {
        [self.rgb[0] + self.master, self.rgb[1] + self.master, self.rgb[2] + self.master]
    }
    pub fn is_neutral(&self) -> bool {
        self.rgb == [0.0; 3] && self.master == 0.0
    }
}

/// Custom curves of one corrector: luma (Y) and the three channels. An empty list is the
/// straight line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CurveSet {
    pub y: Vec<CurvePoint>,
    pub r: Vec<CurvePoint>,
    pub g: Vec<CurvePoint>,
    pub b: Vec<CurvePoint>,
}

impl CurveSet {
    pub fn is_neutral(&self) -> bool {
        [&self.y, &self.r, &self.g, &self.b].iter().all(|c| crate::adjust::is_identity_curve(c) || c.is_empty())
    }
}

/// The five "x versus y" curves of Resolve's Curves page. Each is a list of `[x, change]` points
/// (x in 0..=1 along the curve's own axis, change in -1..=1, zero meaning no change); no points is
/// no change. Hue is a circle, so the hue-axis curves wrap round.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HueCurves {
    /// Hue turned by `change × 180°`.
    pub hue_vs_hue: Vec<[f32; 2]>,
    /// Saturation multiplied by `1 + change`.
    pub hue_vs_sat: Vec<[f32; 2]>,
    /// Lightness raised by `change × 0.5`.
    pub hue_vs_lum: Vec<[f32; 2]>,
    /// Saturation multiplied by `1 + change`, by lightness.
    pub lum_vs_sat: Vec<[f32; 2]>,
    /// Saturation multiplied by `1 + change`, by saturation.
    pub sat_vs_sat: Vec<[f32; 2]>,
}

impl HueCurves {
    pub fn is_neutral(&self) -> bool {
        [&self.hue_vs_hue, &self.hue_vs_sat, &self.hue_vs_lum, &self.lum_vs_sat, &self.sat_vs_sat].iter().all(|c| c.iter().all(|p| p[1] == 0.0))
    }
}

/// The HSL qualifier: picks the pixels whose hue, saturation and lightness fall in the ranges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Qualifier {
    /// Hue range centre (0..=1 round the wheel) and half-width (0..=0.5).
    pub hue_center: f32,
    pub hue_width: f32,
    /// Softness added outside each range (0..=1 of the axis).
    pub hue_soft: f32,
    pub sat: [f32; 2],
    pub sat_soft: f32,
    pub lum: [f32; 2],
    pub lum_soft: f32,
    /// Whether the hue axis is used (a qualifier of only saturation / lightness leaves it off).
    pub use_hue: bool,
    pub use_sat: bool,
    pub use_lum: bool,
    /// Selects everything except the range.
    pub invert: bool,
}

impl Default for Qualifier {
    fn default() -> Self {
        Qualifier {
            hue_center: 0.0,
            hue_width: 0.5,
            hue_soft: 0.0,
            sat: [0.0, 1.0],
            sat_soft: 0.0,
            lum: [0.0, 1.0],
            lum_soft: 0.0,
            use_hue: false,
            use_sat: false,
            use_lum: false,
            invert: false,
        }
    }
}

impl Qualifier {
    /// Whether it selects every pixel (nothing is switched on, or the ranges cover everything).
    pub fn is_open(&self) -> bool {
        !self.invert && !self.use_hue && !self.use_sat && !self.use_lum
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowShape {
    /// An ellipse.
    #[default]
    Circle,
    /// A rectangle.
    Linear,
    /// A band that fades from full on one side to nothing on the other.
    Gradient,
}

/// A power window: a soft-edged shape on the picture. Positions are fractions of the canvas
/// (0..=1), so a window keeps its place when the canvas is resized.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradeWindow {
    pub shape: WindowShape,
    pub center: [f32; 2],
    /// Width and height as fractions of the canvas.
    pub size: [f32; 2],
    /// Degrees, clockwise.
    pub rotation: f32,
    /// How far the edge fades, as a fraction of the window's smaller size (0..=1).
    pub softness: f32,
    /// Selects the outside instead.
    pub invert: bool,
    pub enabled: bool,
}

impl Default for GradeWindow {
    fn default() -> Self {
        GradeWindow { shape: WindowShape::Circle, center: [0.5, 0.5], size: [0.5, 0.5], rotation: 0.0, softness: 0.1, invert: false, enabled: true }
    }
}

/// What limits a node to part of the picture: a qualifier and windows (their keys multiply), and
/// a gain on the result (the node's opacity).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Matte {
    pub qualifier: Qualifier,
    pub windows: Vec<GradeWindow>,
    /// Opacity of the node's effect, 0..=1.
    pub gain: f32,
}

impl Default for Matte {
    fn default() -> Self {
        Matte { qualifier: Qualifier::default(), windows: Vec::new(), gain: 1.0 }
    }
}

impl Matte {
    /// Whether the node acts on every pixel at full strength.
    pub fn is_open(&self) -> bool {
        self.qualifier.is_open() && self.gain >= 1.0 && self.windows.iter().all(|w| !w.enabled)
    }
}

/// The primary controls and curves of a corrector node, and the matte that limits it.
/// Neutral values: wheels at zero, contrast and saturation 1, pivot 0.435, the rest 0.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corrector {
    /// Dark end: pushes the black point (the picture's shadows).
    pub lift: Wheel,
    /// The middle: bends the mid-tones (a power).
    pub gamma: Wheel,
    /// The bright end: multiplies the picture.
    pub gain: Wheel,
    /// Adds the same amount everywhere.
    pub offset: Wheel,
    /// Contrast about `pivot` (1 = none).
    pub contrast: f32,
    pub pivot: f32,
    /// 1 = none, 0 = grey, 2 = double.
    pub saturation: f32,
    /// Degrees the hues are turned.
    pub hue: f32,
    /// Warm (+) or cool (-), -1..=1.
    pub temperature: f32,
    /// Magenta (+) or green (-), -1..=1.
    pub tint: f32,
    /// Raises the saturation of dull colours more than vivid ones, -1..=1.
    pub boost: f32,
    /// Lifts or sinks the dark part of the picture, -1..=1.
    pub shadows: f32,
    /// Lifts or sinks the bright part, -1..=1.
    pub highlights: f32,
    pub curves: CurveSet,
    pub hue_curves: HueCurves,
    pub matte: Matte,
}

impl Default for Corrector {
    fn default() -> Self {
        Corrector {
            lift: Wheel::default(),
            gamma: Wheel::default(),
            gain: Wheel::default(),
            offset: Wheel::default(),
            contrast: 1.0,
            pivot: 0.435,
            saturation: 1.0,
            hue: 0.0,
            temperature: 0.0,
            tint: 0.0,
            boost: 0.0,
            shadows: 0.0,
            highlights: 0.0,
            curves: CurveSet::default(),
            hue_curves: HueCurves::default(),
            matte: Matte::default(),
        }
    }
}

impl Corrector {
    /// Whether the corrector changes nothing (its matte aside).
    pub fn is_neutral(&self) -> bool {
        self.lift.is_neutral()
            && self.gamma.is_neutral()
            && self.gain.is_neutral()
            && self.offset.is_neutral()
            && self.contrast == 1.0
            && self.saturation == 1.0
            && self.hue == 0.0
            && self.temperature == 0.0
            && self.tint == 0.0
            && self.boost == 0.0
            && self.shadows == 0.0
            && self.highlights == 0.0
            && self.curves.is_neutral()
            && self.hue_curves.is_neutral()
    }
}

/// A 3D LUT applied as a node.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LutNode {
    pub name: String,
    /// `size`³ RGB triplets, red varying fastest, 0..=1 (the `ColorLookup` layout). None: no change.
    pub lut: Option<Arc<Vec<f32>>>,
    pub size: u32,
    /// How much of the LUT's change is applied, 0..=1.
    pub intensity: f32,
    pub tetrahedral: bool,
    pub matte: Matte,
}

/// A LUT can be 100 000 numbers, and layers' `Debug` text is what the renderer's cache key is made
/// from: so the table shows as its size and a checksum.
impl std::fmt::Debug for LutNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sum = self.lut.as_ref().map(|t| t.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, v| (h ^ u64::from(v.to_bits())).wrapping_mul(0x100_0000_01b3)));
        f.debug_struct("LutNode")
            .field("name", &self.name)
            .field("size", &self.size)
            .field("entries", &self.lut.as_ref().map(|t| t.len()))
            .field("checksum", &sum)
            .field("intensity", &self.intensity)
            .field("tetrahedral", &self.tetrahedral)
            .field("matte", &self.matte)
            .finish()
    }
}

/// How a layer mixer lays one input over the ones below it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixLayer {
    pub mode: BlendMode,
    /// 0..=1.
    pub opacity: f32,
}

impl Default for MixLayer {
    fn default() -> Self {
        MixLayer { mode: BlendMode::Normal, opacity: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    /// Reads one input and grades it.
    Corrector(Box<Corrector>),
    /// Joins parallel branches: the result is `base + Σ(branch − base)`, so every branch's change
    /// to the shared `base` (the node they all grew from; None: the source) is added.
    ParallelMixer { base: Option<NodeId> },
    /// Lays its inputs over each other, the first at the bottom; `layers[i]` belongs to `inputs[i]`
    /// (the bottom one's mode is ignored). An input's matte is its key.
    LayerMixer { layers: Vec<MixLayer> },
    /// Reads one input and runs it through a LUT.
    Lut(Box<LutNode>),
}

impl NodeKind {
    pub fn is_mixer(&self) -> bool {
        matches!(self, NodeKind::ParallelMixer { .. } | NodeKind::LayerMixer { .. })
    }
    /// The matte of a node that has one.
    pub fn matte(&self) -> Option<&Matte> {
        match self {
            NodeKind::Corrector(c) => Some(&c.matte),
            NodeKind::Lut(l) => Some(&l.matte),
            _ => None,
        }
    }
    pub fn matte_mut(&mut self) -> Option<&mut Matte> {
        match self {
            NodeKind::Corrector(c) => Some(&mut c.matte),
            NodeKind::Lut(l) => Some(&mut l.matte),
            _ => None,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            NodeKind::Corrector(_) => "corrector",
            NodeKind::ParallelMixer { .. } => "parallelMixer",
            NodeKind::LayerMixer { .. } => "layerMixer",
            NodeKind::Lut(_) => "lut",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradeNode {
    pub id: NodeId,
    pub label: String,
    /// Where the editor draws it (its own units; set by [`NodeGraph::auto_layout`]).
    pub pos: [f32; 2],
    /// Passes the input through unchanged (a mixer passes its first input).
    pub bypass: bool,
    /// What it reads: one entry for a corrector or LUT, two or more for a mixer. `None` is the
    /// layer's source image.
    pub inputs: Vec<Option<NodeId>>,
    pub kind: NodeKind,
}

/// Why a graph is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphError(pub String);

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GraphError {}

fn err<T>(msg: impl Into<String>) -> Result<T, GraphError> {
    Err(GraphError(msg.into()))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeGraph {
    pub nodes: Vec<GradeNode>,
    /// The node whose result is the layer's.
    pub output: NodeId,
    /// The id the next new node takes.
    pub next_id: NodeId,
}

impl Default for NodeGraph {
    /// One corrector node between the source and the output, as Resolve starts a clip.
    fn default() -> Self {
        NodeGraph {
            nodes: vec![GradeNode { id: 1, label: "01".into(), pos: [0.0, 0.0], bypass: false, inputs: vec![None], kind: NodeKind::Corrector(Box::default()) }],
            output: 1,
            next_id: 2,
        }
    }
}

impl NodeGraph {
    pub fn node(&self, id: NodeId) -> Option<&GradeNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut GradeNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// The nodes in an order where each comes after the ones it reads (the ones the output does
    /// not reach are left out). An error for a loop or a missing input node.
    pub fn order(&self) -> Result<Vec<NodeId>, GraphError> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            Visiting,
            Done,
        }
        fn visit(g: &NodeGraph, id: NodeId, marks: &mut Vec<(NodeId, Mark)>, out: &mut Vec<NodeId>) -> Result<(), GraphError> {
            match marks.iter().find(|m| m.0 == id).map(|m| m.1) {
                Some(Mark::Done) => return Ok(()),
                Some(Mark::Visiting) => return err("the nodes form a loop"),
                None => {}
            }
            let Some(node) = g.node(id) else { return err(format!("node {id} does not exist")) };
            marks.push((id, Mark::Visiting));
            for input in node.inputs.iter().flatten() {
                visit(g, *input, marks, out)?;
            }
            if let NodeKind::ParallelMixer { base: Some(b) } = &node.kind {
                visit(g, *b, marks, out)?;
            }
            for m in marks.iter_mut().filter(|m| m.0 == id) {
                m.1 = Mark::Done;
            }
            out.push(id);
            Ok(())
        }
        let mut out = Vec::new();
        visit(self, self.output, &mut Vec::new(), &mut out)?;
        Ok(out)
    }

    /// Checks everything a graph must satisfy to be evaluated.
    pub fn validate(&self) -> Result<(), GraphError> {
        if self.nodes.is_empty() || self.nodes.len() > MAX_NODES {
            return err(format!("a graph has 1 to {MAX_NODES} nodes, not {}", self.nodes.len()));
        }
        for (i, n) in self.nodes.iter().enumerate() {
            if self.nodes[..i].iter().any(|m| m.id == n.id) {
                return err(format!("node id {} is used twice", n.id));
            }
            if n.id >= self.next_id {
                return err(format!("node {} is not below next_id", n.id));
            }
            let base = if let NodeKind::ParallelMixer { base: Some(b) } = &n.kind { Some(*b) } else { None };
            if let Some(missing) = n.inputs.iter().flatten().copied().chain(base).find(|i| self.node(*i).is_none()) {
                return err(format!("node {} reads node {missing}, which does not exist", n.id));
            }
            let (min, max) = match &n.kind {
                NodeKind::Corrector(_) | NodeKind::Lut(_) => (1, 1),
                NodeKind::ParallelMixer { .. } | NodeKind::LayerMixer { .. } => (1, MAX_INPUTS),
            };
            if n.inputs.len() < min || n.inputs.len() > max {
                return err(format!("node {} reads {} inputs, expected {min} to {max}", n.id, n.inputs.len()));
            }
            if let NodeKind::LayerMixer { layers } = &n.kind
                && layers.len() != n.inputs.len()
            {
                return err(format!("layer mixer {} has {} layer settings for {} inputs", n.id, layers.len(), n.inputs.len()));
            }
            if let Some(m) = n.kind.matte()
                && (m.windows.len() > MAX_WINDOWS || !m.gain.is_finite())
            {
                return err(format!("node {} has a bad matte", n.id));
            }
            if let NodeKind::Lut(l) = &n.kind
                && let Some(t) = &l.lut
                && (l.size < 2 || l.size > MAX_LUT_SIZE || t.len() < (l.size as usize).pow(3) * 3)
            {
                return err(format!("node {}'s LUT is malformed", n.id));
            }
        }
        if self.node(self.output).is_none() {
            return err("the output node does not exist");
        }
        if self.order_all_has_loop() {
            return err("the nodes form a loop");
        }
        self.order().map(|_| ())
    }

    fn fresh_id(&mut self) -> Result<NodeId, GraphError> {
        if self.nodes.len() >= MAX_NODES {
            return err(format!("a graph holds at most {MAX_NODES} nodes"));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        Ok(id)
    }

    /// Whether a new branch fits: the graph has room for the new node (and a mixer when none
    /// joins it yet), and the mixer that does has an input free.
    fn check_room(&self, joined: Option<NodeId>) -> Result<(), GraphError> {
        match joined {
            Some(m) => {
                if self.node(m).is_some_and(|n| n.inputs.len() >= MAX_INPUTS) {
                    return err(format!("a mixer reads at most {MAX_INPUTS} inputs"));
                }
                if self.nodes.len() >= MAX_NODES {
                    return err(format!("a graph holds at most {MAX_NODES} nodes"));
                }
            }
            None => {
                if self.nodes.len() + 2 > MAX_NODES {
                    return err(format!("a graph holds at most {MAX_NODES} nodes"));
                }
            }
        }
        Ok(())
    }

    fn label_for(&self, id: NodeId) -> String {
        format!("{id:02}")
    }

    /// A new corrector right after `after` (None: right after the source): whatever read `after`
    /// now reads the new node. Returns its id.
    pub fn add_serial(&mut self, after: Option<NodeId>) -> Result<NodeId, GraphError> {
        if let Some(a) = after
            && self.node(a).is_none()
        {
            return err(format!("node {a} does not exist"));
        }
        let id = self.fresh_id()?;
        for n in &mut self.nodes {
            for input in n.inputs.iter_mut().filter(|i| **i == after) {
                *input = Some(id);
            }
            if let NodeKind::ParallelMixer { base } = &mut n.kind
                && *base == after
            {
                *base = Some(id);
            }
        }
        let label = self.label_for(id);
        self.nodes.push(GradeNode { id, label, pos: [0.0, 0.0], bypass: false, inputs: vec![after], kind: NodeKind::Corrector(Box::default()) });
        // Added at the end of the chain, the new node is the end; added at the source or in the
        // middle, the output stays where it was.
        if Some(self.output) == after {
            self.output = id;
        }
        self.auto_layout();
        Ok(id)
    }

    /// A new corrector that reads what `of` reads, side by side with it, joined to it by a
    /// parallel mixer that takes `of`'s place. Returns the new corrector's id.
    pub fn add_parallel(&mut self, of: NodeId) -> Result<NodeId, GraphError> {
        let Some(node) = self.node(of) else { return err(format!("node {of} does not exist")) };
        if node.kind.is_mixer() {
            return err("a mixer has no parallel node; add one to a corrector");
        }
        let upstream = node.inputs.first().copied().flatten();
        // Already joined by a mixer: the new branch goes into it.
        let joined = self.nodes.iter().find(|n| matches!(n.kind, NodeKind::ParallelMixer { .. }) && n.inputs.contains(&Some(of))).map(|n| n.id);
        self.check_room(joined)?;
        let new = self.fresh_id()?;
        let label = self.label_for(new);
        self.nodes.push(GradeNode { id: new, label, pos: [0.0, 0.0], bypass: false, inputs: vec![upstream], kind: NodeKind::Corrector(Box::default()) });
        match joined {
            Some(m) => {
                if let Some(mixer) = self.node_mut(m) {
                    mixer.inputs.push(Some(new));
                }
            }
            None => {
                let mixer = self.fresh_id()?;
                let label = self.label_for(mixer);
                for n in &mut self.nodes {
                    if n.id == new {
                        continue;
                    }
                    for input in n.inputs.iter_mut().filter(|i| **i == Some(of)) {
                        *input = Some(mixer);
                    }
                }
                self.nodes.push(GradeNode { id: mixer, label, pos: [0.0, 0.0], bypass: false, inputs: vec![Some(of), Some(new)], kind: NodeKind::ParallelMixer { base: upstream } });
                if self.output == of {
                    self.output = mixer;
                }
            }
        }
        self.auto_layout();
        Ok(new)
    }

    /// A new layer mixer that lays `top` over `bottom` (the new corrector reading `bottom`'s
    /// input is not made: `top` is a fresh corrector reading the same source as `bottom`).
    /// Returns the new corrector's id.
    pub fn add_layer(&mut self, bottom: NodeId) -> Result<NodeId, GraphError> {
        let Some(node) = self.node(bottom) else { return err(format!("node {bottom} does not exist")) };
        let upstream = node.inputs.first().copied().flatten();
        let joined = self.nodes.iter().find(|n| matches!(n.kind, NodeKind::LayerMixer { .. }) && n.inputs.contains(&Some(bottom))).map(|n| n.id);
        self.check_room(joined)?;
        let new = self.fresh_id()?;
        let label = self.label_for(new);
        self.nodes.push(GradeNode { id: new, label, pos: [0.0, 0.0], bypass: false, inputs: vec![upstream], kind: NodeKind::Corrector(Box::default()) });
        match joined {
            Some(m) => {
                if let Some(mixer) = self.node_mut(m) {
                    mixer.inputs.push(Some(new));
                    if let NodeKind::LayerMixer { layers } = &mut mixer.kind {
                        layers.push(MixLayer::default());
                    }
                }
            }
            None => {
                let mixer = self.fresh_id()?;
                let label = self.label_for(mixer);
                for n in &mut self.nodes {
                    if n.id == new {
                        continue;
                    }
                    for input in n.inputs.iter_mut().filter(|i| **i == Some(bottom)) {
                        *input = Some(mixer);
                    }
                }
                self.nodes.push(GradeNode {
                    id: mixer,
                    label,
                    pos: [0.0, 0.0],
                    bypass: false,
                    inputs: vec![Some(bottom), Some(new)],
                    kind: NodeKind::LayerMixer { layers: vec![MixLayer::default(); 2] },
                });
                if self.output == bottom {
                    self.output = mixer;
                }
            }
        }
        self.auto_layout();
        Ok(new)
    }

    /// Removes a node: what read it reads what it read (its first input). A mixer left with one
    /// input is dissolved too. The last node cannot be removed.
    pub fn remove(&mut self, id: NodeId) -> Result<(), GraphError> {
        if self.nodes.len() <= 1 {
            return err("the last node cannot be removed");
        }
        let Some(pos) = self.nodes.iter().position(|n| n.id == id) else { return err(format!("node {id} does not exist")) };
        let gone = self.nodes.remove(pos);
        let replacement = gone.inputs.first().copied().flatten();
        for n in &mut self.nodes {
            // Inputs that read the removed node: a mixer drops the branch, anything else reads on.
            if n.kind.is_mixer() && n.inputs.len() > 1 {
                let keep: Vec<usize> = (0..n.inputs.len()).filter(|i| n.inputs[*i] != Some(id)).collect();
                if keep.len() != n.inputs.len() && !keep.is_empty() {
                    n.inputs = keep.iter().map(|i| n.inputs[*i]).collect();
                    if let NodeKind::LayerMixer { layers } = &mut n.kind {
                        *layers = keep.iter().map(|i| layers.get(*i).copied().unwrap_or_default()).collect();
                    }
                }
            }
            for input in n.inputs.iter_mut().filter(|i| **i == Some(id)) {
                *input = replacement;
            }
            if let NodeKind::ParallelMixer { base } = &mut n.kind
                && *base == Some(id)
            {
                *base = replacement;
            }
        }
        if self.output == id {
            self.output = match replacement {
                Some(r) => r,
                None => self.nodes.last().map_or(0, |n| n.id),
            };
        }
        // A mixer with a single input just passes it on.
        let lonely: Vec<NodeId> = self.nodes.iter().filter(|n| n.kind.is_mixer() && n.inputs.len() == 1).map(|n| n.id).collect();
        for m in lonely {
            if self.nodes.len() > 1 {
                let _ = self.remove(m);
            }
        }
        self.auto_layout();
        Ok(())
    }

    /// Makes `slot` of node `to` read node `from` (None: the source). A loop is refused.
    pub fn connect(&mut self, to: NodeId, slot: usize, from: Option<NodeId>) -> Result<(), GraphError> {
        if let Some(f) = from {
            if self.node(f).is_none() {
                return err(format!("node {f} does not exist"));
            }
            if f == to {
                return err("a node cannot read itself");
            }
        }
        let before = self.clone();
        let Some(node) = self.node_mut(to) else { return err(format!("node {to} does not exist")) };
        let Some(input) = node.inputs.get_mut(slot) else { return err(format!("node {to} has no input {slot}")) };
        *input = from;
        if self.order_all_has_loop() {
            *self = before;
            return err("that connection would make the nodes form a loop");
        }
        self.auto_layout();
        Ok(())
    }

    /// Whether following the inputs from any node comes back to it (the output's own subtree is
    /// checked by [`order`](Self::order); this also catches a loop in nodes the output skips).
    fn order_all_has_loop(&self) -> bool {
        let mut probe = self.clone();
        for n in &self.nodes {
            probe.output = n.id;
            if probe.order().is_err() {
                return true;
            }
        }
        false
    }

    /// Sets the output node.
    pub fn set_output(&mut self, id: NodeId) -> Result<(), GraphError> {
        if self.node(id).is_none() {
            return err(format!("node {id} does not exist"));
        }
        self.output = id;
        Ok(())
    }

    /// Places the nodes in columns by how far they are from the source, and spreads those of one
    /// column down. Only moves nodes; the structure stays.
    pub fn auto_layout(&mut self) {
        let mut depth: Vec<(NodeId, usize)> = Vec::new();
        fn depth_of(g: &NodeGraph, id: NodeId, seen: &mut Vec<NodeId>) -> usize {
            if seen.contains(&id) {
                return 0;
            }
            seen.push(id);
            let d = g.node(id).map_or(0, |n| 1 + n.inputs.iter().flatten().map(|i| depth_of(g, *i, seen)).max().unwrap_or(0));
            seen.pop();
            d
        }
        for n in &self.nodes {
            depth.push((n.id, depth_of(self, n.id, &mut Vec::new())));
        }
        let mut rows: Vec<usize> = Vec::new();
        for (id, d) in &depth {
            if rows.len() <= *d {
                rows.resize(*d + 1, 0);
            }
            let row = rows[*d];
            rows[*d] += 1;
            if let Some(n) = self.node_mut(*id) {
                n.pos = [(*d as f32 - 1.0) * 1.0, row as f32];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(g: &NodeGraph) -> Vec<NodeId> {
        g.nodes.iter().map(|n| n.id).collect()
    }

    #[test]
    fn the_default_graph_is_one_neutral_corrector() {
        let g = NodeGraph::default();
        assert!(g.validate().is_ok());
        assert_eq!(g.order(), Ok(vec![1]));
        let NodeKind::Corrector(c) = &g.nodes[0].kind else { return };
        assert!(c.is_neutral() && c.matte.is_open());
    }

    #[test]
    fn serial_nodes_chain() {
        let mut g = NodeGraph::default();
        let two = g.add_serial(Some(1)).unwrap();
        let three = g.add_serial(Some(two)).unwrap();
        assert_eq!((two, three), (2, 3));
        assert_eq!(g.output, 3);
        assert_eq!(g.order().unwrap(), vec![1, 2, 3]);
        // Inserted in the middle: the node after it now reads the new node.
        let mid = g.add_serial(Some(1)).unwrap();
        assert_eq!(g.order().unwrap(), vec![1, mid, 2, 3]);
        // At the source: everything that read the source reads the new node.
        let first = g.add_serial(None).unwrap();
        assert_eq!(g.node(1).unwrap().inputs, vec![Some(first)]);
        assert!(g.validate().is_ok());
    }

    #[test]
    fn parallel_nodes_share_an_input_and_join_in_a_mixer() {
        let mut g = NodeGraph::default();
        let b = g.add_parallel(1).unwrap();
        // 1 and b both read the source; a mixer reads both and is now the output.
        assert_eq!(g.node(b).unwrap().inputs, vec![None]);
        let mixer = g.output;
        assert!(matches!(g.node(mixer).unwrap().kind, NodeKind::ParallelMixer { base: None }));
        assert_eq!(g.node(mixer).unwrap().inputs, vec![Some(1), Some(b)]);
        // A third branch joins the same mixer.
        let c = g.add_parallel(1).unwrap();
        assert_eq!(g.node(mixer).unwrap().inputs, vec![Some(1), Some(b), Some(c)]);
        assert!(g.validate().is_ok());
        // Removing a branch leaves the others joined; removing down to one dissolves the mixer.
        g.remove(c).unwrap();
        g.remove(b).unwrap();
        assert_eq!(ids(&g), vec![1]);
        assert_eq!(g.output, 1);
    }

    #[test]
    fn layers_join_in_a_layer_mixer() {
        let mut g = NodeGraph::default();
        let top = g.add_layer(1).unwrap();
        let mixer = g.output;
        let NodeKind::LayerMixer { layers } = &g.node(mixer).unwrap().kind else { return };
        assert_eq!(layers.len(), 2);
        assert_eq!(g.node(mixer).unwrap().inputs, vec![Some(1), Some(top)]);
        g.add_layer(1).unwrap();
        let NodeKind::LayerMixer { layers } = &g.node(mixer).unwrap().kind else { return };
        assert_eq!(layers.len(), 3);
        assert!(g.validate().is_ok());
    }

    #[test]
    fn removing_a_node_reconnects_its_neighbours() {
        let mut g = NodeGraph::default();
        let two = g.add_serial(Some(1)).unwrap();
        let three = g.add_serial(Some(two)).unwrap();
        g.remove(two).unwrap();
        assert_eq!(g.node(three).unwrap().inputs, vec![Some(1)]);
        g.remove(three).unwrap();
        assert_eq!(g.output, 1);
        assert!(g.remove(1).is_err(), "the last node stays");
        assert!(g.remove(99).is_err());
    }

    #[test]
    fn loops_and_bad_references_are_refused() {
        let mut g = NodeGraph::default();
        let two = g.add_serial(Some(1)).unwrap();
        assert!(g.connect(1, 0, Some(two)).is_err(), "1 -> 2 -> 1 is a loop");
        assert!(g.connect(two, 0, Some(two)).is_err());
        assert!(g.connect(two, 0, Some(77)).is_err());
        assert!(g.connect(two, 3, None).is_err());
        assert_eq!(g.node(1).unwrap().inputs, vec![None], "a refused connection changes nothing");
        assert!(g.connect(two, 0, None).is_ok());
        // Hand-made damage is caught by validate.
        let mut bad = g.clone();
        bad.nodes[0].inputs = vec![Some(1)];
        assert!(bad.validate().is_err(), "a node reading itself");
        let mut bad = g.clone();
        bad.nodes[0].inputs = vec![Some(55)];
        assert!(bad.validate().is_err(), "a missing node");
        let mut bad = g.clone();
        bad.output = 99;
        assert!(bad.validate().is_err());
        let mut bad = g.clone();
        bad.nodes.push(bad.nodes[0].clone());
        assert!(bad.validate().is_err(), "duplicate id");
        let mut bad = g.clone();
        bad.nodes[0].inputs.clear();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn the_node_count_is_limited() {
        let mut g = NodeGraph::default();
        for _ in 1..MAX_NODES {
            let last = g.output;
            g.add_serial(Some(last)).unwrap();
        }
        assert_eq!(g.nodes.len(), MAX_NODES);
        assert!(g.add_serial(Some(g.output)).is_err());
        assert!(g.validate().is_ok());
    }

    #[test]
    fn a_graph_round_trips_through_json() {
        let mut g = NodeGraph::default();
        g.add_parallel(1).unwrap();
        if let Some(NodeKind::Corrector(c)) = g.node_mut(1).map(|n| &mut n.kind) {
            c.lift.master = 0.2;
            c.hue_curves.hue_vs_sat = vec![[0.1, 0.5]];
            c.matte.windows.push(GradeWindow::default());
        }
        let text = serde_json::to_string(&g).unwrap();
        let back: NodeGraph = serde_json::from_str(&text).unwrap();
        assert_eq!(back, g);
    }
}
