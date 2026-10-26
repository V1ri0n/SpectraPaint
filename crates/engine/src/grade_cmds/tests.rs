use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"width": 8, "height": 8, "background": "white"})).unwrap();
    s.execute("layer.newFillLayer.solidColor", json!({"color": "#404040"})).unwrap();
    s
}

fn new_grade_layer(s: &mut Session) -> u64 {
    s.execute("grade.new", json!({})).unwrap()["layer"].as_u64().unwrap()
}

fn graph(s: &mut Session) -> NodeGraph {
    serde_json::from_value(s.execute("grade.info", json!({})).unwrap()["graph"].clone()).unwrap()
}

fn pixel(s: &Session) -> [f32; 3] {
    let b = photocraft_compose::flatten(&s.active().unwrap().doc);
    [b.px[0][0], b.px[0][1], b.px[0][2]]
}

fn history(s: &Session) -> usize {
    s.active().unwrap().history.past_len()
}

#[test]
fn a_grade_layer_is_made_and_undone() {
    let mut s = session();
    let layers = s.active().unwrap().doc.layer_count();
    let id = new_grade_layer(&mut s);
    assert_eq!(s.active().unwrap().doc.layer_count(), layers + 1);
    assert_eq!(s.active().unwrap().active_layer, Some(LayerId(id)));
    let info = s.execute("grade.info", json!({})).unwrap();
    assert_eq!(info["summary"]["output"], 1);
    assert_eq!(info["summary"]["nodes"][0]["kind"], "corrector");
    assert!(s.undo());
    assert_eq!(s.active().unwrap().doc.layer_count(), layers);
    // Commands for a grade layer are not available on another layer.
    assert!(s.execute("grade.info", json!({})).is_err());
}

#[test]
fn settings_change_what_the_picture_looks_like() {
    let mut s = session();
    new_grade_layer(&mut s);
    let before = pixel(&s);
    s.execute("grade.setNode", json!({"node": 1, "set": {"gain": {"master": 1.0}}})).unwrap();
    let after = pixel(&s);
    assert!((after[0] - before[0] * 2.0).abs() < 0.01, "gain doubled {before:?} to {after:?}");
    // Bypassed, the picture is as before; one history step per change.
    let steps = history(&s);
    s.execute("grade.setNode", json!({"node": 1, "bypass": true})).unwrap();
    assert_eq!(history(&s), steps + 1);
    assert!((pixel(&s)[0] - before[0]).abs() < 1e-4);
    s.execute("grade.setNode", json!({"node": 1, "bypass": false, "label": "Look"})).unwrap();
    assert_eq!(graph(&mut s).nodes[0].label, "Look");
    // Undo steps back through the changes.
    assert!(s.undo() && s.undo());
    assert!((pixel(&s)[0] - after[0]).abs() < 1e-4);
}

#[test]
fn bad_settings_are_refused_and_change_nothing() {
    let mut s = session();
    new_grade_layer(&mut s);
    let steps = history(&s);
    let before = graph(&mut s);
    for p in [
        json!({"node": 1, "set": {"nonsense": 1}}),
        json!({"node": 1, "set": {"gain": {"colour": 1}}}),
        json!({"node": 1, "set": {"contrast": "very"}}),
        json!({"node": 1, "set": 5}),
        json!({"node": 1}),
        json!({"node": 9, "set": {"contrast": 1.2}}),
        json!({"set": {"contrast": 1.2}}),
        json!({"node": 1, "label": ""}),
        json!({"node": 1, "bypass": "yes"}),
    ] {
        assert!(s.execute("grade.setNode", p.clone()).is_err(), "{p}");
    }
    let too_many: Vec<Value> = (0..=MAX_CURVE_POINTS).map(|i| json!({"input": i as f64 / 100.0, "output": 0.5})).collect();
    assert!(s.execute("grade.setNode", json!({"node": 1, "set": {"curves": {"y": too_many}}})).is_err());
    assert_eq!(history(&s), steps);
    assert_eq!(graph(&mut s), before);
}

#[test]
fn nodes_are_added_connected_and_removed() {
    let mut s = session();
    new_grade_layer(&mut s);
    let two = s.execute("grade.addNode", json!({"kind": "serial"})).unwrap()["node"].as_u64().unwrap();
    assert_eq!(two, 2);
    assert_eq!(graph(&mut s).output, 2);
    let par = s.execute("grade.addNode", json!({"kind": "parallel", "node": 2})).unwrap()["node"].as_u64().unwrap() as u32;
    let g = graph(&mut s);
    assert!(matches!(g.node(g.output).unwrap().kind, NodeKind::ParallelMixer { .. }), "the mixer is the output now");
    s.execute("grade.addNode", json!({"kind": "layer", "node": 1})).unwrap();
    // A loop is refused with nothing changed.
    let steps = history(&s);
    let err = s.execute("grade.connect", json!({"node": 1, "from": 2})).unwrap_err().to_string();
    assert!(err.contains("loop"), "{err}");
    assert_eq!(history(&s), steps);
    s.execute("grade.connect", json!({"node": par, "from": null})).unwrap();
    s.execute("grade.setOutput", json!({"node": 1})).unwrap();
    assert_eq!(graph(&mut s).output, 1);
    s.execute("grade.removeNode", json!({"node": par})).unwrap();
    assert!(graph(&mut s).node(par).is_none());
    assert!(s.execute("grade.removeNode", json!({"node": 99})).is_err());
    assert!(s.execute("grade.addNode", json!({"kind": "sideways"})).is_err());
    assert!(s.execute("grade.addNode", json!({"kind": "serial", "after": 77})).is_err());
}

#[test]
fn parallel_and_layer_nodes_change_the_picture() {
    let mut s = session();
    new_grade_layer(&mut s);
    let base = pixel(&s)[0];
    let b = s.execute("grade.addNode", json!({"kind": "parallel", "node": 1})).unwrap()["node"].as_u64().unwrap();
    s.execute("grade.setNode", json!({"node": 1, "set": {"offset": {"master": 0.2}}})).unwrap();
    s.execute("grade.setNode", json!({"node": b, "set": {"offset": {"master": 0.2}}})).unwrap();
    // Each branch adds 0.1 to the shared input.
    assert!((pixel(&s)[0] - (base + 0.2)).abs() < 0.01, "{} vs {}", pixel(&s)[0], base + 0.2);
    // Resetting a node puts it back.
    s.execute("grade.resetNode", json!({"node": b})).unwrap();
    assert!((pixel(&s)[0] - (base + 0.1)).abs() < 0.01);
}

#[test]
fn a_look_can_be_saved_applied_and_kept_between_runs() {
    let mut s = session();
    new_grade_layer(&mut s);
    s.execute("grade.setNode", json!({"node": 1, "set": {"saturation": 0.0, "lift": {"rgb": [0.1, 0.0, 0.0]}}})).unwrap();
    s.execute("grade.look.save", json!({"name": "Mono red"})).unwrap();
    let second = new_grade_layer(&mut s);
    assert!(graph(&mut s).node(1).is_some_and(|n| matches!(&n.kind, NodeKind::Corrector(c) if c.saturation == 1.0)));
    s.execute("grade.look.apply", json!({"name": "Mono red"})).unwrap();
    assert!(graph(&mut s).node(1).is_some_and(|n| matches!(&n.kind, NodeKind::Corrector(c) if c.saturation == 0.0 && c.lift.rgb[0] == 0.1)));
    // Saving under a name again replaces; the list shows one.
    s.execute("grade.look.save", json!({"name": "Mono red"})).unwrap();
    assert_eq!(s.execute("grade.look.list", json!({})).unwrap()["looks"].as_array().map(Vec::len), Some(1));
    // The preferences carry it to the next run, and a damaged entry is left behind.
    let saved = s.presets.to_json(&s);
    let mut next = Session::new();
    next.load_presets_json(saved);
    assert_eq!(next.presets.grade_looks.len(), 1);
    assert_eq!(next.presets.grade_looks[0].name, "Mono red");
    let mut blob = next.presets.to_json(&next);
    blob["grade_looks"].as_array_mut().unwrap().push(json!({"name": "broken", "graph": {"nodes": []}}));
    let mut again = Session::new();
    again.load_presets_json(blob);
    assert_eq!(again.presets.grade_looks.len(), 1);
    // copyTo puts a grade on another Color Grade layer.
    let third = new_grade_layer(&mut s);
    s.execute("grade.copyTo", json!({"from": second, "to": third})).unwrap();
    assert_eq!(graph(&mut s), { s.execute("grade.info", json!({"layer": second})).map(|v| serde_json::from_value::<NodeGraph>(v["graph"].clone()).unwrap()).unwrap() });
    assert!(s.execute("grade.look.apply", json!({"name": "nope"})).is_err());
    // A look travels as a text file and comes back under a free name.
    let shared = s.execute("grade.look.export", json!({"name": "Mono red"})).unwrap();
    let text = shared["data"].as_str().unwrap().to_string();
    let r = s.execute("grade.look.import", json!({"data": text.clone()})).unwrap();
    assert_eq!(r["name"], "Mono red 2");
    assert!(s.execute("grade.look.import", json!({"data": "{}"})).is_err());
    assert!(s.execute("grade.look.import", json!({"data": text.replace("spectra-grade-look", "other")})).is_err());
    assert!(s.execute("grade.look.import", json!({})).is_err());
    assert!(s.execute("grade.look.export", json!({"name": "nope"})).is_err());
    s.execute("grade.look.delete", json!({"name": "Mono red 2"})).unwrap();
    s.execute("grade.look.delete", json!({"name": "Mono red"})).unwrap();
    assert!(s.execute("grade.look.delete", json!({"name": "Mono red"})).is_err());
}

#[test]
fn luts_come_in_and_the_grade_goes_out_as_a_lut() {
    let mut s = session();
    new_grade_layer(&mut s);
    // An inverting 2³ .cube.
    let cube = "LUT_3D_SIZE 2\n1 1 1\n0 1 1\n1 0 1\n0 0 1\n1 1 0\n0 1 0\n1 0 0\n0 0 0\n";
    let before = pixel(&s)[0];
    let r = s.execute("grade.addLut", json!({"data": cube, "fileName": "invert.cube"})).unwrap();
    let node = r["node"].as_u64().unwrap() as u32;
    assert!(matches!(graph(&mut s).node(node).unwrap().kind, NodeKind::Lut(_)));
    assert!((pixel(&s)[0] - (1.0 - before)).abs() < 0.01, "inverted");
    s.execute("grade.setNode", json!({"node": node, "set": {"intensity": 0.0}})).unwrap();
    assert!((pixel(&s)[0] - before).abs() < 0.01);
    assert!(s.execute("grade.addLut", json!({})).is_err(), "a LUT is needed");
    assert!(s.execute("grade.addLut", json!({"data": "nonsense", "fileName": "x.cube"})).is_err());
    // Out again: the grade baked to a .cube that reads back the same.
    s.execute("grade.setNode", json!({"node": node, "set": {"intensity": 1.0}})).unwrap();
    let out = s.execute("grade.exportLut", json!({"size": 5})).unwrap();
    assert_eq!(out["size"], 5);
    let text = out["data"].as_str().unwrap();
    let back = photocraft_cms::lutfile::parse("x.cube", text.as_bytes()).unwrap();
    assert_eq!(back.size, 5);
    // Black went in, white came out.
    assert!(back.data[..3].iter().all(|v| *v > 0.7), "{:?}", &back.data[..3]);
    assert!(s.execute("grade.exportLut", json!({"size": 1})).is_err());
}

#[test]
fn a_grade_goes_to_psd_as_a_baked_color_lookup_with_a_note() {
    let mut s = session();
    new_grade_layer(&mut s);
    s.execute("grade.setNode", json!({"node": 1, "set": {"gain": {"master": 0.5}}})).unwrap();
    let doc = &s.active().unwrap().doc;
    let r = photocraft_io::export(doc, "x.psd", &Default::default()).unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("Color Grade") && w.contains("Color Lookup")), "{:?}", r.warnings);
    let back = photocraft_io::import("x.psd", &r.bytes).unwrap().document;
    let looked = back.layers.iter().any(|l| matches!(&l.content, LayerContent::Adjustment(Adjustment::ColorLookup { size: 33, .. })));
    assert!(looked, "the layer reads back as a Color Lookup");
    // The graph itself is what a .pcraft keeps.
    let bytes = photocraft_io::export(doc, "x.pcraft", &Default::default()).unwrap().bytes;
    let again = photocraft_io::import("x.pcraft", &bytes).unwrap().document;
    assert!(again.layers.iter().any(|l| matches!(&l.content, LayerContent::Adjustment(Adjustment::NodeGrade { graph }) if graph.nodes.len() == 1)));
}

#[test]
fn the_plain_adjustment_commands_leave_a_grade_alone() {
    let mut s = session();
    new_grade_layer(&mut s);
    let err = s.execute("layer.setAdjustment", json!({"brightness": 10})).map(|_| ()).unwrap_err().to_string();
    assert!(err.contains("grade."), "{err}");
}

/// A picture with a green background and a red layer over the left half; the red layer is active.
fn two_layers() -> (Session, u64) {
    let mut s = Session::new();
    s.execute("file.new", json!({"width": 8, "height": 4, "background": "white"})).unwrap();
    s.execute("layer.newFillLayer.solidColor", json!({"color": "#00ff00"})).unwrap();
    let red = s.execute("layer.new.layer", json!({})).unwrap()["layer"].as_u64().unwrap();
    s.execute("select.rect", json!({"x": 0, "y": 0, "width": 4, "height": 4})).unwrap();
    s.execute("edit.fill", json!({"contents": "color", "color": "#ff0000"})).unwrap();
    s.execute("select.deselect", json!({})).unwrap();
    (s, red)
}

fn at(s: &Session, x: usize, y: usize) -> [f32; 3] {
    let b = photocraft_compose::flatten(&s.active().unwrap().doc);
    let p = b.px[y * 8 + x];
    [p[0], p[1], p[2]]
}

#[test]
fn a_grade_made_on_a_layer_is_clipped_to_it_and_grades_only_that_layer() {
    let (mut s, red) = two_layers();
    let r = s.execute("grade.new", json!({})).unwrap();
    assert_eq!(r["clipped"], true);
    let grade = LayerId(r["layer"].as_u64().unwrap());
    let doc = &s.active().unwrap().doc;
    assert!(doc.layer(grade).unwrap().clipped);
    // Right above the layer it grades.
    let order: Vec<u64> = doc.layers.iter().map(|l| l.id.0).collect();
    let (i_red, i_grade) = (order.iter().position(|&i| i == red).unwrap(), order.iter().position(|&i| i == grade.0).unwrap());
    assert_eq!(i_grade, i_red + 1);
    let (green, red_px) = (at(&s, 6, 1), at(&s, 1, 1));
    s.execute("grade.setNode", json!({"node": 1, "set": {"offset": {"rgb": [0.0, 0.0, 1.0], "master": 0.0}}})).unwrap();
    assert_eq!(at(&s, 6, 1), green, "the layers the grade is not clipped to keep their colour");
    assert_ne!(at(&s, 1, 1), red_px, "the layer it is clipped to changes");
}

#[test]
fn a_global_grade_goes_on_top_and_grades_everything() {
    let (mut s, _red) = two_layers();
    let r = s.execute("grade.newGlobal", json!({})).unwrap();
    assert_eq!(r["clipped"], false);
    let grade = LayerId(r["layer"].as_u64().unwrap());
    let doc = &s.active().unwrap().doc;
    assert_eq!(doc.layers.last().map(|l| l.id), Some(grade), "on top of the stack");
    assert!(!doc.layer(grade).unwrap().clipped);
    assert!(doc.layer(grade).unwrap().name.starts_with("Global Grade"));
    let green = at(&s, 6, 1);
    s.execute("grade.setNode", json!({"node": 1, "set": {"offset": {"rgb": [0.0, 0.0, 1.0], "master": 0.0}}})).unwrap();
    assert_ne!(at(&s, 6, 1), green, "every layer below is graded");
    // One history step each, and undo takes the layer away.
    assert!(s.undo() && s.undo());
    assert!(s.active().unwrap().doc.layer(grade).is_none());
}

#[test]
fn a_grade_made_while_a_grade_is_active_is_global_unless_that_one_is_clipped() {
    let (mut s, _red) = two_layers();
    // A global grade is active: a new one has nothing to attach to.
    let g = s.execute("grade.newGlobal", json!({})).unwrap()["layer"].as_u64().unwrap();
    let again = s.execute("grade.new", json!({})).unwrap();
    assert_eq!(again["clipped"], false);
    assert_ne!(again["layer"].as_u64().unwrap(), g);
    // With nothing active (an empty document) the grade is plain and global.
    let mut empty = Session::new();
    empty.execute("file.new", json!({"width": 4, "height": 4, "background": "white"})).unwrap();
    assert!(empty.execute("grade.new", json!({})).is_ok());
}
