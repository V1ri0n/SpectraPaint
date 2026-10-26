//! Colour harmonies (`swatches.harmony`): from one colour, the colours that go with it by a
//! classic rule of the colour wheel (complementary, analogous, triadic, …), or a ramp of its
//! shades, tints and tones. The colours come back as hex, and can be put into the Swatches panel
//! as a new group, which is how a colour theme is saved.
//!
//! The wheel is the HSB one the Colors window shows: a harmony turns the hue and keeps the
//! saturation and brightness, so the colours of a rule look as related as the base is vivid.

use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::presets::swatches::{Swatch, SwatchColor, SwatchConv, SwatchKind, hex, hsb_to_rgb};
use crate::presets::{Group, bad, unique_name};
use crate::swatch_cmds::{MAX_GROUPS, MAX_NAME, MAX_SWATCHES};
use crate::{Result, Session};

/// Most colours a ramp (analogous, monochromatic, shades, tints, tones) has.
pub const MAX_COUNT: usize = 24;

/// The rules, as `(id, English name)`. The Colors window lists them in this order.
pub const RULES: [(&str, &str); 10] = [
    ("complementary", "Complementary"),
    ("splitComplementary", "Split complementary"),
    ("analogous", "Analogous"),
    ("triadic", "Triadic"),
    ("tetradic", "Tetradic"),
    ("square", "Square"),
    ("monochromatic", "Monochromatic"),
    ("shades", "Shades"),
    ("tints", "Tints"),
    ("tones", "Tones"),
];

/// RGB (0..1) to HSB with the hue in degrees.
pub fn rgb_to_hsb([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    [h, if max <= f32::EPSILON { 0.0 } else { d / max }, max]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// The colours of `rule` for `base`, the base itself first (ramps keep it where the rule puts it).
/// `count` only matters to the rules that have no fixed number of colours.
pub fn harmony(base: [f32; 3], rule: &str, count: usize) -> Option<Vec<[f32; 3]>> {
    let count = count.clamp(2, MAX_COUNT);
    let [h, s, v] = rgb_to_hsb(base);
    let turn = |degrees: &[f32]| degrees.iter().map(|d| hsb_to_rgb([h + d, s, v])).collect::<Vec<_>>();
    Some(match rule {
        "complementary" => turn(&[0.0, 180.0]),
        "splitComplementary" => turn(&[0.0, 150.0, 210.0]),
        "triadic" => turn(&[0.0, 120.0, 240.0]),
        "tetradic" => turn(&[0.0, 60.0, 180.0, 240.0]),
        "square" => turn(&[0.0, 90.0, 180.0, 270.0]),
        // Neighbours at 30° steps, centred on the base: the base is in the middle (or just left of it).
        "analogous" => {
            let mid = (count - 1) / 2;
            (0..count).map(|i| hsb_to_rgb([h + 30.0 * (i as f32 - mid as f32), s, v])).collect()
        }
        // Towards black and towards white from the base, which sits at its place in between.
        "monochromatic" => {
            let mid = (count - 1) / 2;
            (0..count)
                .map(|i| {
                    if i < mid {
                        mix(base, [0.0; 3], 0.8 * (mid - i) as f32 / mid as f32)
                    } else if i > mid {
                        mix(base, [1.0; 3], 0.8 * (i - mid) as f32 / (count - 1 - mid) as f32)
                    } else {
                        base
                    }
                })
                .collect()
        }
        "shades" => (0..count).map(|i| mix(base, [0.0; 3], 0.85 * i as f32 / (count - 1) as f32)).collect(),
        "tints" => (0..count).map(|i| mix(base, [1.0; 3], 0.85 * i as f32 / (count - 1) as f32)).collect(),
        "tones" => (0..count).map(|i| mix(base, [0.5; 3], 0.85 * i as f32 / (count - 1) as f32)).collect(),
        _ => return None,
    })
}

fn harmony_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "swatches.harmony";
    let base = match p.get("color") {
        None | Some(Value::Null) => {
            let f = s.tools.foreground;
            [f[0], f[1], f[2]].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 })
        }
        Some(v) => {
            let c = SwatchColor::from_param(v).map_err(|e| bad(CMD, e))?;
            SwatchConv::new(s).rgb(&c)
        }
    };
    let rule = p.get("rule").and_then(Value::as_str).unwrap_or("complementary");
    let count = match p.get("count") {
        None | Some(Value::Null) => 5,
        Some(v) => v.as_u64().filter(|n| (2..=MAX_COUNT as u64).contains(n)).ok_or_else(|| bad(CMD, format!("`count` is 2..{MAX_COUNT}")))? as usize,
    };
    let colors = harmony(base, rule, count)
        .ok_or_else(|| bad(CMD, format!("unknown rule `{rule}` ({})", RULES.iter().map(|r| r.0).collect::<Vec<_>>().join("|"))))?;
    let hexes: Vec<String> = colors.iter().map(|c| hex(*c)).collect();
    if !p.get("addToSwatches").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(json!({"rule": rule, "colors": hexes}));
    }
    // A theme: the colours as a new swatch group (named as asked, else "<Rule> <base hex>").
    let total: usize = s.presets.swatches.iter().map(|g| g.items.len()).sum();
    if total + colors.len() > MAX_SWATCHES || s.presets.swatches.len() >= MAX_GROUPS {
        return Err(bad(CMD, "the Swatches panel is full"));
    }
    let label = RULES.iter().find(|r| r.0 == rule).map_or(rule, |r| r.1);
    let group_name = match p.get("group") {
        None | Some(Value::Null) => format!("{label} {}", hexes[0]),
        Some(Value::String(g)) if !g.trim().is_empty() => g.trim().chars().take(MAX_NAME).collect(),
        Some(_) => return Err(bad(CMD, "`group` must be a non-empty string")),
    };
    let group_name = crate::presets::gradients::unique_group(&s.presets.swatches, &group_name);
    let mut items = Vec::with_capacity(colors.len());
    for (c, h) in colors.iter().zip(&hexes) {
        // Names stay unique across the library, and across the new group too.
        let taken = |n: &str| items.iter().any(|i: &Swatch| i.name == n);
        let mut name = unique_name(&s.presets.swatches, h);
        while taken(&name) {
            name = format!("{name} {}", items.len() + 1);
        }
        items.push(Swatch { name, color: SwatchColor::Rgb(*c), kind: SwatchKind::Process });
    }
    s.presets.swatches.push(Group::new(&group_name, items));
    s.presets_changed();
    Ok(json!({"rule": rule, "colors": hexes, "group": group_name}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "swatches.harmony",
        label: "Color Harmony…",
        menu: &[],
        shortcut: None,
        params: r##"{"color":"#rrggbb"|{model,values}?=foreground,"rule":"complementary|splitComplementary|analogous|triadic|tetradic|square|monochromatic|shades|tints|tones"="complementary","count":2..24=5 (ramps only),"addToSwatches":bool=false (save as a new swatch group),"group":str?} → {rule,colors:[hex],group?}"##,
        enabled: |_| Ok(()),
        run: harmony_cmd,
        journal: true,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hexes(base: [f32; 3], rule: &str, n: usize) -> Vec<String> {
        harmony(base, rule, n).unwrap().iter().map(|c| hex(*c)).collect()
    }

    #[test]
    fn hsb_round_trip() {
        for c in [[1.0, 0.0, 0.0], [0.2, 0.6, 0.9], [0.5, 0.5, 0.5], [0.0, 0.0, 0.0], [0.9, 0.8, 0.1]] {
            let back = hsb_to_rgb(rgb_to_hsb(c));
            for i in 0..3 {
                assert!((back[i] - c[i]).abs() < 1e-5, "{c:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn hue_rules_turn_the_wheel() {
        let red = [1.0, 0.0, 0.0];
        assert_eq!(hexes(red, "complementary", 5), ["#ff0000", "#00ffff"]);
        assert_eq!(hexes(red, "triadic", 5), ["#ff0000", "#00ff00", "#0000ff"]);
        assert_eq!(hexes(red, "square", 5), ["#ff0000", "#80ff00", "#00ffff", "#8000ff"]);
        assert_eq!(hexes(red, "splitComplementary", 5), ["#ff0000", "#00ff80", "#0080ff"]);
        assert_eq!(hexes(red, "tetradic", 5).len(), 4);
    }

    #[test]
    fn analogous_is_centred_on_the_base() {
        let c = hexes([1.0, 0.0, 0.0], "analogous", 5);
        assert_eq!(c.len(), 5);
        assert_eq!(c[2], "#ff0000");
        assert_eq!(c[1], "#ff0080");
        assert_eq!(c[3], "#ff8000");
    }

    #[test]
    fn ramps_keep_the_base_and_stay_in_range() {
        let base = [0.2, 0.5, 0.8];
        assert_eq!(hexes(base, "shades", 4)[0], hex(base));
        assert_eq!(hexes(base, "tints", 4)[0], hex(base));
        let mono = harmony(base, "monochromatic", 7).unwrap();
        assert_eq!(mono.len(), 7);
        assert_eq!(hex(mono[3]), hex(base));
        // Darker to the left of the base, lighter to the right.
        assert!(mono[0].iter().sum::<f32>() < mono[3].iter().sum::<f32>());
        assert!(mono[6].iter().sum::<f32>() > mono[3].iter().sum::<f32>());
        assert!(harmony(base, "nope", 3).is_none());
        // Counts are clamped, a grey has no hue to turn and stays grey.
        assert_eq!(harmony(base, "tints", 1000).unwrap().len(), MAX_COUNT);
        assert_eq!(hexes([0.5; 3], "triadic", 3), ["#808080"; 3]);
    }

    #[test]
    fn command_returns_colors_and_saves_a_theme() {
        let mut s = Session::new();
        let r = s.execute("swatches.harmony", json!({"color": "#ff0000", "rule": "triadic"})).unwrap();
        assert_eq!(r["colors"], json!(["#ff0000", "#00ff00", "#0000ff"]));
        assert!(r.get("group").is_none());
        let before = s.presets.swatches.len();
        // From the foreground colour when none is given; saved as a group.
        s.tools.foreground = [0.0, 0.0, 1.0, 1.0];
        let r = s.execute("swatches.harmony", json!({"rule": "complementary", "addToSwatches": true, "group": "Sea"})).unwrap();
        assert_eq!(r["group"], "Sea");
        assert_eq!(s.presets.swatches.len(), before + 1);
        let g = s.presets.swatches.last().unwrap();
        assert_eq!(g.name, "Sea");
        assert_eq!(g.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["#0000ff", "#ffff00"]);
        // The same theme again gets a fresh group and fresh names.
        let r = s.execute("swatches.harmony", json!({"rule": "complementary", "addToSwatches": true, "group": "Sea"})).unwrap();
        assert_ne!(r["group"], "Sea");
        let names: Vec<_> = s.presets.swatches.iter().flat_map(|g| g.items.iter().map(|i| i.name.clone())).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "swatch names stay unique");
        // Bad input is refused, not guessed at.
        assert!(s.execute("swatches.harmony", json!({"rule": "nope"})).is_err());
        assert!(s.execute("swatches.harmony", json!({"rule": "tints", "count": 1})).is_err());
        assert!(s.execute("swatches.harmony", json!({"color": "red"})).is_err());
    }
}
