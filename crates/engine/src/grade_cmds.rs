//! Node-based colour grading (`grade.*`): the commands that make and edit a Color Grade
//! adjustment layer, whose graph of nodes ([`photocraft_doc::grade`]) is evaluated by
//! `photocraft_compose::grade`.
//!
//! A node is addressed by its id (`grade.info` lists them); the layer is `layer`, else the active
//! layer. Settings of a node are changed with `grade.setNode`, whose `set` object is merged into
//! the node's own settings (the same names `grade.info` shows), so one command reaches every wheel,
//! curve, qualifier and window. Every change is one history step; a graph that would not be valid
//! is refused and nothing changes.
//!
//! Saved grades ("looks") live with the other presets and are kept between runs.

use photocraft_doc::grade::{LutNode, MAX_CURVE_POINTS, Matte, MixLayer, NodeGraph, NodeId, NodeKind};
use photocraft_doc::{Adjustment, Document, Layer, LayerContent, LayerId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, layer_param, mask_new_layer};
use crate::presets::{bad, req_str, str_param};
use crate::{EngineError, Result, Session};

/// Most saved looks.
pub const MAX_LOOKS: usize = 200;
/// Longest look name (characters).
const MAX_NAME: usize = 120;

/// A saved grade.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradeLook {
    pub name: String,
    pub graph: NodeGraph,
}

/// Reads saved looks one by one: an entry that does not read is dropped, the rest stay.
pub fn load_looks(v: Value) -> Vec<GradeLook> {
    let Value::Array(items) = v else { return Vec::new() };
    items
        .into_iter()
        .filter_map(|i| serde_json::from_value::<GradeLook>(i).ok())
        .filter(|l| !l.name.is_empty() && l.name.chars().count() <= MAX_NAME && l.graph.validate().is_ok())
        .take(MAX_LOOKS)
        .collect()
}

fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}

fn has_grade(s: &Session) -> std::result::Result<(), String> {
    let d = s.active().ok_or("no document open")?;
    let id = d.active_layer.ok_or("no active layer")?;
    match d.doc.layer(id).map(|l| &l.content) {
        Some(LayerContent::Adjustment(Adjustment::NodeGrade { .. })) => Ok(()),
        _ => Err("the active layer is not a Color Grade layer".into()),
    }
}

/// The Color Grade layer a command acts on.
fn target(s: &Session, p: &Value, cmd: &str) -> Result<LayerId> {
    let id = layer_param(s, p)?;
    let doc = &s.active().ok_or(EngineError::NoDocument)?.doc;
    match doc.layer(id).map(|l| &l.content) {
        Some(LayerContent::Adjustment(Adjustment::NodeGrade { .. })) => Ok(id),
        Some(_) => Err(bad(cmd, format!("layer {} is not a Color Grade layer (grade.new makes one)", id.0))),
        None => Err(EngineError::NoLayer(id)),
    }
}

fn graph_in(doc: &Document, id: LayerId) -> Result<&NodeGraph> {
    match doc.layer(id).map(|l| &l.content) {
        Some(LayerContent::Adjustment(Adjustment::NodeGrade { graph })) => Ok(graph),
        _ => Err(EngineError::NoLayer(id)),
    }
}

fn graph_mut(doc: &mut Document, id: LayerId) -> Result<&mut NodeGraph> {
    match doc.layer_mut(id).map(|l| &mut l.content) {
        Some(LayerContent::Adjustment(Adjustment::NodeGrade { graph })) => Ok(graph),
        _ => Err(EngineError::NoLayer(id)),
    }
}

/// Settings no node may have: curves with too many points, numbers that are not numbers.
fn sanity(g: &NodeGraph, cmd: &str) -> Result<()> {
    g.validate().map_err(|e| bad(cmd, e.0))?;
    for n in &g.nodes {
        let NodeKind::Corrector(c) = &n.kind else { continue };
        let curves = [&c.curves.y, &c.curves.r, &c.curves.g, &c.curves.b];
        let hue = [&c.hue_curves.hue_vs_hue, &c.hue_curves.hue_vs_sat, &c.hue_curves.hue_vs_lum, &c.hue_curves.lum_vs_sat, &c.hue_curves.sat_vs_sat];
        if curves.iter().any(|v| v.len() > MAX_CURVE_POINTS) || hue.iter().any(|v| v.len() > MAX_CURVE_POINTS) {
            return Err(bad(cmd, format!("node {}: a curve has more than {MAX_CURVE_POINTS} points", n.id)));
        }
        let numbers = [
            c.contrast, c.pivot, c.saturation, c.hue, c.temperature, c.tint, c.boost, c.shadows, c.highlights, c.matte.gain, c.lift.master, c.gamma.master, c.gain.master,
            c.offset.master,
        ];
        if numbers.iter().any(|v| !v.is_finite()) || [c.lift, c.gamma, c.gain, c.offset].iter().any(|w| w.rgb.iter().any(|v| !v.is_finite())) {
            return Err(bad(cmd, format!("node {}: a setting is not a finite number", n.id)));
        }
    }
    Ok(())
}

/// Changes the graph of the Color Grade layer `p` names with `f` as one history step. A graph the
/// change would break is refused and the document stays as it was.
fn edit_graph<R>(s: &mut Session, p: &Value, cmd: &str, label: &str, f: impl FnOnce(&mut NodeGraph) -> Result<R>) -> Result<(LayerId, R)> {
    let id = target(s, p, cmd)?;
    let r = s.edit(label, |doc, _| {
        let graph = graph_mut(doc, id)?;
        let mut g = graph.clone();
        let r = f(&mut g)?;
        sanity(&g, cmd)?;
        *graph = g;
        Ok(r)
    })?;
    Ok((id, r))
}

fn node_param(p: &Value, key: &str, cmd: &str) -> Result<NodeId> {
    p.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| NodeId::try_from(n).ok())
        .ok_or_else(|| bad(cmd, format!("`{key}` must be a node id (see grade.info)")))
}

/// `null` or a node id: the source image, or a node.
fn source_param(p: &Value, key: &str, cmd: &str) -> Result<Option<NodeId>> {
    match p.get(key) {
        Some(Value::Null) => Ok(None),
        Some(_) => node_param(p, key, cmd).map(Some),
        None => Err(bad(cmd, format!("`{key}` must be a node id, or null for the source image"))),
    }
}

fn summary(g: &NodeGraph) -> Value {
    json!({
        "output": g.output,
        "nodes": g.nodes.iter().map(|n| json!({"id": n.id, "label": n.label, "kind": n.kind.name(), "inputs": n.inputs, "bypass": n.bypass})).collect::<Vec<_>>(),
    })
}

/// Merges `patch` into `base`: objects key by key (a key `base` lacks is an error naming it),
/// anything else replaces.
fn merge(base: &mut Value, patch: &Value, path: &str) -> std::result::Result<(), String> {
    match (base, patch) {
        (Value::Object(b), Value::Object(pa)) => {
            for (k, v) in pa {
                match b.get_mut(k) {
                    Some(bv) => merge(bv, v, &format!("{path}.{k}"))?,
                    None => return Err(format!("this node has no setting `{path}.{k}`")),
                }
            }
            Ok(())
        }
        (b, pa) => {
            *b = pa.clone();
            Ok(())
        }
    }
}

// ------------------------------------------------------------------ commands

fn new_grade(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.new";
    let name = str_param(p, "name").map(str::to_string);
    let id = s.edit("New Color Grade Layer", |doc, active| {
        let name = name.unwrap_or_else(|| doc.next_layer_name("Color Grade"));
        let mut l = Layer::new(name, LayerContent::Adjustment(Adjustment::NodeGrade { graph: NodeGraph::default() }));
        mask_new_layer(doc, &mut l, p, CMD)?;
        let id = doc.insert_above(*active, l);
        *active = Some(id);
        Ok(id)
    })?;
    Ok(json!({"layer": id.0}))
}

fn info(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "grade.info")?;
    let g = graph_in(&s.active().ok_or(EngineError::NoDocument)?.doc, id)?;
    Ok(json!({"layer": id.0, "graph": g, "summary": summary(g)}))
}

fn set_graph(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.setGraph";
    let graph: NodeGraph = serde_json::from_value(p.get("graph").cloned().ok_or_else(|| bad(CMD, "missing `graph`"))?).map_err(|e| bad(CMD, format!("`graph`: {e}")))?;
    let (id, _) = edit_graph(s, p, CMD, "Set Color Grade", |g| {
        *g = graph;
        Ok(())
    })?;
    Ok(json!({"layer": id.0}))
}

fn add_node(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.addNode";
    let kind = str_param(p, "kind").unwrap_or("serial");
    let (id, node) = edit_graph(s, p, CMD, "Add Grade Node", |g| {
        let res = match kind {
            "serial" => {
                let after = if p.get("after").is_some() { source_param(p, "after", CMD)? } else { Some(g.output) };
                g.add_serial(after)
            }
            "parallel" => g.add_parallel(if p.get("node").is_some() { node_param(p, "node", CMD)? } else { g.output }),
            "layer" => g.add_layer(if p.get("node").is_some() { node_param(p, "node", CMD)? } else { g.output }),
            other => return Err(bad(CMD, format!("unknown `kind` `{other}` (serial|parallel|layer)"))),
        };
        res.map_err(|e| bad(CMD, e.0))
    })?;
    Ok(json!({"layer": id.0, "node": node}))
}

fn remove_node(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.removeNode";
    let node = node_param(p, "node", CMD)?;
    let (id, _) = edit_graph(s, p, CMD, "Remove Grade Node", |g| g.remove(node).map_err(|e| bad(CMD, e.0)))?;
    Ok(json!({"layer": id.0}))
}

fn connect(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.connect";
    let node = node_param(p, "node", CMD)?;
    let input = p.get("input").map_or(Some(0), Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(CMD, "`input` must be a number"))?;
    let from = source_param(p, "from", CMD)?;
    let (id, _) = edit_graph(s, p, CMD, "Connect Grade Nodes", |g| g.connect(node, input, from).map_err(|e| bad(CMD, e.0)))?;
    Ok(json!({"layer": id.0}))
}

fn set_output(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.setOutput";
    let node = node_param(p, "node", CMD)?;
    let (id, _) = edit_graph(s, p, CMD, "Set Grade Output", |g| g.set_output(node).map_err(|e| bad(CMD, e.0)))?;
    Ok(json!({"layer": id.0}))
}

fn set_node(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.setNode";
    let node = node_param(p, "node", CMD)?;
    let label = match p.get("label") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) if !t.trim().is_empty() && t.chars().count() <= 60 => Some(t.trim().to_string()),
        Some(_) => return Err(bad(CMD, "`label` must be a name of up to 60 characters")),
    };
    let bypass = match p.get("bypass") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad(CMD, "`bypass` must be true or false"))?),
    };
    let set = p.get("set");
    if let Some(v) = set
        && !v.is_object()
    {
        return Err(bad(CMD, "`set` must be an object of settings"));
    }
    if label.is_none() && bypass.is_none() && set.is_none() {
        return Err(bad(CMD, "nothing to change: give `set`, `label` or `bypass`"));
    }
    let (id, _) = edit_graph(s, p, CMD, "Grade Node", |g| {
        let n = g.node_mut(node).ok_or_else(|| bad(CMD, format!("node {node} does not exist")))?;
        if let Some(l) = label {
            n.label = l;
        }
        if let Some(b) = bypass {
            n.bypass = b;
        }
        if let Some(patch) = set {
            let mut v = serde_json::to_value(&n.kind).map_err(|e| bad(CMD, e.to_string()))?;
            // `{"Corrector": {…settings…}}`: the settings are the one entry's value.
            let inner = v.as_object_mut().and_then(|m| m.values_mut().next()).ok_or_else(|| bad(CMD, "the node has no settings"))?;
            merge(inner, patch, "").map_err(|e| bad(CMD, e))?;
            n.kind = serde_json::from_value(v).map_err(|e| bad(CMD, format!("the settings do not fit the node: {e}")))?;
        }
        Ok(())
    })?;
    Ok(json!({"layer": id.0}))
}

fn reset_node(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.resetNode";
    let node = node_param(p, "node", CMD)?;
    let (id, _) = edit_graph(s, p, CMD, "Reset Grade Node", |g| {
        let n = g.node_mut(node).ok_or_else(|| bad(CMD, format!("node {node} does not exist")))?;
        n.kind = match &n.kind {
            NodeKind::Corrector(_) => NodeKind::Corrector(Box::default()),
            NodeKind::Lut(_) => NodeKind::Lut(Box::new(LutNode { intensity: 1.0, ..Default::default() })),
            NodeKind::ParallelMixer { base } => NodeKind::ParallelMixer { base: *base },
            NodeKind::LayerMixer { layers } => NodeKind::LayerMixer { layers: vec![MixLayer::default(); layers.len()] },
        };
        n.bypass = false;
        Ok(())
    })?;
    Ok(json!({"layer": id.0}))
}

/// A LUT node after the output (or after `after`), read from a file, the text of one, or a
/// built-in look: the params are those of Color Lookup.
fn add_lut(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.addLut";
    let Adjustment::ColorLookup { name, lut, size, tetrahedral, .. } = crate::adjust_cmds::lookup_from_params(p, None)? else {
        return Err(bad(CMD, "no LUT given"));
    };
    if lut.is_none() {
        return Err(bad(CMD, "give a LUT: `file`, `data` (with `fileName`) or `lut` (a built-in look)"));
    }
    let intensity = match p.get("intensity") {
        None | Some(Value::Null) => 1.0,
        Some(v) => v.as_f64().filter(|x| (0.0..=1.0).contains(x)).ok_or_else(|| bad(CMD, "`intensity` is 0..1"))? as f32,
    };
    let (id, node) = edit_graph(s, p, CMD, "Add LUT Node", |g| {
        let after = if p.get("after").is_some() { source_param(p, "after", CMD)? } else { Some(g.output) };
        let new = g.add_serial(after).map_err(|e| bad(CMD, e.0))?;
        if let Some(n) = g.node_mut(new) {
            n.label = name.chars().take(40).collect();
            n.kind = NodeKind::Lut(Box::new(LutNode { name: name.clone(), lut: lut.clone(), size, intensity, tetrahedral, matte: Matte::default() }));
        }
        Ok(new)
    })?;
    Ok(json!({"layer": id.0, "node": node}))
}

/// The grade as a `.cube` 3D LUT: to `path`, else as `data` text.
fn export_lut(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.exportLut";
    let id = target(s, p, CMD)?;
    let size = match p.get("size") {
        None | Some(Value::Null) => 33,
        Some(v) => v.as_u64().filter(|n| (2..=65).contains(n)).ok_or_else(|| bad(CMD, "`size` is 2..65"))? as usize,
    };
    let g = graph_in(&s.active().ok_or(EngineError::NoDocument)?.doc, id)?;
    let windows = photocraft_compose::grade::has_windows(g);
    let data = photocraft_compose::grade::bake_lut(g, size).ok_or_else(|| bad(CMD, "the graph cannot be evaluated"))?;
    let title = str_param(p, "title").unwrap_or("Color Grade").to_string();
    let text = photocraft_cms::lutfile::write_cube(&photocraft_cms::lutfile::LutFile { title, size, data, domain_min: [0.0; 3], domain_max: [1.0; 3] });
    let mut out = json!({"size": size, "windowsLeftOut": windows, "bytes": text.len()});
    match str_param(p, "path") {
        Some(path) => {
            crate::file_cmds::write_file(path, text.as_bytes())?;
            out["path"] = json!(path);
        }
        None => out["data"] = json!(text),
    }
    Ok(out)
}

/// The graph of one Color Grade layer put on another.
fn copy_to(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.copyTo";
    let from_p = match p.get("from") {
        Some(v) if !v.is_null() => json!({"layer": v}),
        _ => Value::Null,
    };
    let from = target(s, &from_p, CMD)?;
    let to = LayerId(p.get("to").and_then(Value::as_u64).ok_or_else(|| bad(CMD, "`to` must be a Color Grade layer id"))?);
    let graph = graph_in(&s.active().ok_or(EngineError::NoDocument)?.doc, from)?.clone();
    if from == to {
        return Ok(json!({"layer": to.0}));
    }
    target(s, &json!({"layer": to.0}), CMD)?;
    s.edit("Copy Color Grade", |doc, _| {
        *graph_mut(doc, to)? = graph;
        Ok(())
    })?;
    Ok(json!({"layer": to.0}))
}

fn clean_name(p: &Value, cmd: &str) -> Result<String> {
    let name = req_str(p, "name", cmd)?;
    if name.chars().count() > MAX_NAME {
        return Err(bad(cmd, format!("a look's name is at most {MAX_NAME} characters")));
    }
    Ok(name.to_string())
}

fn look_save(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.look.save";
    let name = clean_name(p, CMD)?;
    let id = target(s, p, CMD)?;
    let graph = graph_in(&s.active().ok_or(EngineError::NoDocument)?.doc, id)?.clone();
    let looks = &mut s.presets.grade_looks;
    match looks.iter_mut().find(|l| l.name == name) {
        Some(l) => l.graph = graph,
        None => {
            if looks.len() >= MAX_LOOKS {
                return Err(bad(CMD, format!("at most {MAX_LOOKS} looks can be saved")));
            }
            looks.push(GradeLook { name: name.clone(), graph });
        }
    }
    s.presets_changed();
    Ok(json!({"name": name, "count": s.presets.grade_looks.len()}))
}

fn look_apply(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.look.apply";
    let name = clean_name(p, CMD)?;
    let graph = s.presets.grade_looks.iter().find(|l| l.name == name).map(|l| l.graph.clone()).ok_or_else(|| bad(CMD, format!("no look \"{name}\" (grade.look.list)")))?;
    let (id, _) = edit_graph(s, p, CMD, "Apply Grade Look", |g| {
        *g = graph;
        Ok(())
    })?;
    Ok(json!({"layer": id.0}))
}

fn look_delete(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.look.delete";
    let name = clean_name(p, CMD)?;
    let before = s.presets.grade_looks.len();
    s.presets.grade_looks.retain(|l| l.name != name);
    if s.presets.grade_looks.len() == before {
        return Err(bad(CMD, format!("no look \"{name}\"")));
    }
    s.presets_changed();
    Ok(json!({"count": s.presets.grade_looks.len()}))
}

/// The text of a look as a file: what is shared between people.
const LOOK_FORMAT: &str = "spectra-grade-look";

fn look_export(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.look.export";
    let name = clean_name(p, CMD)?;
    let look = s.presets.grade_looks.iter().find(|l| l.name == name).ok_or_else(|| bad(CMD, format!("no look \"{name}\" (grade.look.list)")))?;
    let text = serde_json::to_string_pretty(&json!({"format": LOOK_FORMAT, "version": 1, "name": look.name, "graph": look.graph})).map_err(|e| bad(CMD, e.to_string()))?;
    let mut out = json!({"bytes": text.len()});
    match str_param(p, "path") {
        Some(path) => {
            crate::file_cmds::write_file(path, text.as_bytes())?;
            out["path"] = json!(path);
        }
        None => out["data"] = json!(text),
    }
    Ok(out)
}

/// Reads a look from a file (`path`) or its text (`data`); `name` renames it, and a name already
/// taken gets a number.
fn look_import(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "grade.look.import";
    let text = match (str_param(p, "data"), str_param(p, "path")) {
        (Some(d), _) => d.to_string(),
        (None, Some(path)) => String::from_utf8(crate::file_cmds::read_file(path)?).map_err(|_| bad(CMD, format!("{path} is not text")))?,
        (None, None) => return Err(bad(CMD, "give `path` or `data`")),
    };
    let v: Value = serde_json::from_str(&text).map_err(|e| bad(CMD, format!("not a grade look file: {e}")))?;
    if v.get("format").and_then(Value::as_str) != Some(LOOK_FORMAT) {
        return Err(bad(CMD, "not a grade look file"));
    }
    let graph: NodeGraph = serde_json::from_value(v.get("graph").cloned().unwrap_or(Value::Null)).map_err(|e| bad(CMD, format!("the look's graph: {e}")))?;
    sanity(&graph, CMD)?;
    let base = match str_param(p, "name").or_else(|| v.get("name").and_then(Value::as_str)) {
        Some(n) if !n.trim().is_empty() => n.trim().chars().take(MAX_NAME).collect::<String>(),
        _ => "Look".to_string(),
    };
    if s.presets.grade_looks.len() >= MAX_LOOKS {
        return Err(bad(CMD, format!("at most {MAX_LOOKS} looks can be saved")));
    }
    let taken = |n: &str| s.presets.grade_looks.iter().any(|l| l.name == n);
    let name = if taken(&base) { (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap_or(base) } else { base };
    s.presets.grade_looks.push(GradeLook { name: name.clone(), graph });
    s.presets_changed();
    Ok(json!({"name": name, "count": s.presets.grade_looks.len()}))
}

fn look_list(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"looks": s.presets.grade_looks.iter().map(|l| json!({"name": l.name, "nodes": l.graph.nodes.len()})).collect::<Vec<_>>()}))
}

pub fn specs() -> Vec<CommandSpec> {
    macro_rules! spec {
        ($id:expr, $label:expr, $menu:expr, $params:expr, $enabled:expr, $run:expr) => {
            CommandSpec { id: $id, label: $label, menu: $menu, shortcut: None, params: $params, enabled: $enabled, journal: true, run: $run }
        };
    }
    const LAYER: &str = r##""layer":id? (a Color Grade layer; default the active layer)"##;
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let with_layer = |body: &str| leak(if body.is_empty() { format!("{{{LAYER}}}") } else { format!("{{{LAYER},{body}}}") });
    vec![
        spec!("grade.new", "Color Grade…", &[], leak(format!("{{\"name\":str?,{}}}", crate::commands::NEW_LAYER_PATH)), has_doc, new_grade),
        spec!("grade.info", "Color Grade Info", &[], with_layer(""), has_grade, info),
        spec!("grade.setGraph", "Set Color Grade", &[], with_layer(r##""graph":{nodes,output,next_id} (as grade.info shows it)"##), has_grade, set_graph),
        spec!(
            "grade.addNode",
            "Add Grade Node",
            &[],
            with_layer(r##""kind":"serial|parallel|layer"="serial","after":id|null? (serial: insert after this node, null = at the source; default the output),"node":id? (parallel, layer: the node to branch from; default the output)"##),
            has_grade,
            add_node
        ),
        spec!("grade.removeNode", "Remove Grade Node", &[], with_layer(r##""node":id"##), has_grade, remove_node),
        spec!("grade.connect", "Connect Grade Nodes", &[], with_layer(r##""node":id,"input":int=0,"from":id|null (null: the source image)"##), has_grade, connect),
        spec!("grade.setOutput", "Set Grade Output", &[], with_layer(r##""node":id"##), has_grade, set_output),
        spec!(
            "grade.setNode",
            "Grade Node Settings",
            &[],
            with_layer(
                r##""node":id,"set":{…}? (merged into the node's settings: corrector → lift|gamma|gain|offset {rgb:[r,g,b],master} (-1..1), contrast=1, pivot=0.435, saturation=1, hue° , temperature, tint, boost, shadows, highlights, curves {y,r,g,b:[{input,output}]}, hueCurves {hue_vs_hue,hue_vs_sat,hue_vs_lum,lum_vs_sat,sat_vs_sat:[[x,change]]}, matte {gain, qualifier{…}, windows[{shape:Circle|Linear|Gradient,center,size,rotation,softness,invert,enabled}]}; layer mixer → layers[{mode,opacity}]; LUT → intensity, tetrahedral),"label":str?,"bypass":bool?"##
            ),
            has_grade,
            set_node
        ),
        spec!("grade.resetNode", "Reset Grade Node", &[], with_layer(r##""node":id"##), has_grade, reset_node),
        spec!(
            "grade.addLut",
            "Add LUT Node…",
            &[],
            with_layer(r##""file":path?|"data":text? (+"fileName")|"lut":built-in look id?,"intensity":0..1=1,"after":id|null?,"interpolation":"trilinear|tetrahedral"?"##),
            has_grade,
            add_lut
        ),
        spec!("grade.exportLut", "Export Grade as LUT…", &[], with_layer(r##""size":2..65=33,"path":file? (else the .cube text comes back as `data`),"title":str?"##), has_grade, export_lut),
        spec!("grade.copyTo", "Copy Grade to Layer", &[], r##"{"from":id? (default the active layer),"to":id (a Color Grade layer)}"##, has_grade, copy_to),
        spec!("grade.look.save", "Save Grade Look", &[], with_layer(r##""name":str (an existing look of that name is replaced)"##), has_grade, look_save),
        spec!("grade.look.apply", "Apply Grade Look", &[], with_layer(r##""name":str"##), has_grade, look_apply),
        spec!("grade.look.export", "Export Grade Look…", &[], r##"{"name":str,"path":file? (else the text comes back as `data`)}"##, has_doc, look_export),
        spec!("grade.look.import", "Import Grade Look…", &[], r##"{"path":file|"data":text,"name":str? (default the file's)}"##, has_doc, look_import),
        spec!("grade.look.delete", "Delete Grade Look", &[], r##"{"name":str}"##, has_doc, look_delete),
        spec!("grade.look.list", "Grade Looks", &[], "{}", has_doc, look_list),
    ]
}

#[cfg(test)]
mod tests;
