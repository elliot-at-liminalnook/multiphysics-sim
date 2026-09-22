use crate::{Rendered, Size, canvas::*};
use serde::{Deserialize, Serialize};
use serde_json::json;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub size: Size,
    pub fit: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            size: Size::default(),
            fit: true,
        }
    }
}
#[derive(Clone)]
pub struct Port {
    pub position: [f32; 2],
    pub label: String,
    pub color: [u8; 3],
}
#[derive(Clone)]
pub struct Card {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub ports: Vec<Port>,
    pub selected: bool,
}
#[derive(Clone)]
pub struct Edge {
    pub id: String,
    pub points: Vec<[f32; 2]>,
    pub color: [u8; 3],
    pub selected: bool,
}
#[derive(Clone)]
pub struct Snapshot {
    pub cards: Vec<Card>,
    pub regions: Vec<crate::Region>,
    pub edges: Vec<Edge>,
    pub camera: [f32; 2],
    pub zoom: f32,
    pub metadata: serde_json::Value,
}
pub fn render(snapshot: &Snapshot, options: &Options) -> Result<Rendered, String> {
    let size = options.size.validate()?;
    if snapshot.cards.is_empty() {
        return Err("schematic has no visible components".into());
    }
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for card in &snapshot.cards {
        for a in 0..2 {
            lo[a] = lo[a].min(card.position[a]);
            hi[a] = hi[a].max(card.position[a] + card.size[a]);
        }
    }
    for edge in &snapshot.edges {
        for p in &edge.points {
            for a in 0..2 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
    }
    let zoom = if options.fit {
        ((size.width as f32 - 90.) / (hi[0] - lo[0]).max(1.))
            .min((size.height as f32 - 150.) / (hi[1] - lo[1]).max(1.))
    } else {
        snapshot.zoom
    };
    let center = if options.fit {
        [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5]
    } else {
        snapshot.camera
    };
    let xy = |p: [f32; 2]| {
        if !options.fit {
            return [
                snapshot.camera[0] + p[0] * zoom,
                94. + snapshot.camera[1] + p[1] * zoom,
            ];
        }
        [
            size.width as f32 * 0.5 + (p[0] - center[0]) * zoom,
            94. + (size.height as f32 - 120.) * 0.5 + (p[1] - center[1]) * zoom,
        ]
    };
    let mut c = Canvas::new(size.width, size.height);
    for region in &snapshot.regions {
        let mut a = [f32::INFINITY; 2];
        let mut b = [f32::NEG_INFINITY; 2];
        for card in snapshot
            .cards
            .iter()
            .filter(|c| region.components.contains(&c.id))
        {
            let p = xy(card.position);
            let q = xy([
                card.position[0] + card.size[0],
                card.position[1] + card.size[1],
            ]);
            for i in 0..2 {
                a[i] = a[i].min(p[i] - 12.);
                b[i] = b[i].max(q[i] + 12.);
            }
        }
        if a[0].is_finite() {
            let fill = region.color.map(|v| (v as f32 * 0.09 + 255. * 0.91) as u8);
            c.rect(a[0], a[1], b[0] - a[0], b[1] - a[1], fill);
            for (p, q) in [
                ([a[0], a[1]], [b[0], a[1]]),
                ([b[0], a[1]], [b[0], b[1]]),
                ([b[0], b[1]], [a[0], b[1]]),
                ([a[0], b[1]], [a[0], a[1]]),
            ] {
                c.line(p, q, 1.5, region.color);
            }
            c.text(
                a[0] + 5.,
                a[1] - 20.,
                15.,
                &region.label,
                region.color,
                b[0] - a[0],
            );
        }
    }
    for edge in &snapshot.edges {
        for pair in edge.points.windows(2) {
            c.line(
                xy(pair[0]),
                xy(pair[1]),
                if edge.selected { 3.0 } else { 1.7 },
                edge.color,
            );
        }
    }
    for card in &snapshot.cards {
        let p = xy(card.position);
        let w = card.size[0] * zoom;
        let h = card.size[1] * zoom;
        let border = if card.selected { TEAL } else { [181, 197, 210] };
        c.rect(p[0] - 2., p[1] - 2., w + 4., h + 4., border);
        c.rect(p[0], p[1], w, h, [255, 255, 255]);
        c.rect(p[0], p[1], w, (80. * zoom).min(h), [234, 242, 247]);
        c.text(
            p[0] + 9. * zoom,
            p[1] + 10. * zoom,
            (22. * zoom).clamp(9., 28.),
            &card.label,
            INK,
            w - 18. * zoom,
        );
        c.text(
            p[0] + 9. * zoom,
            p[1] + 44. * zoom,
            (15. * zoom).clamp(8., 20.),
            &card.detail,
            MUTED,
            w - 18. * zoom,
        );
        for (index, region) in snapshot
            .regions
            .iter()
            .filter(|r| r.components.contains(&card.id))
            .enumerate()
        {
            c.rect(p[0] + index as f32 * 4., p[1], 3., h, region.color);
        }
        for port in &card.ports {
            let q = xy(port.position);
            c.dot(q[0], q[1], (4. * zoom).clamp(2., 6.), port.color);
            let left = q[0] < (p[0] + w * 0.5);
            let x = if left {
                p[0] + 9. * zoom
            } else {
                p[0] + w * 0.53
            };
            c.text(
                x,
                q[1] - 8. * zoom,
                (15. * zoom).clamp(8., 20.),
                &port.label,
                port.color,
                w * 0.45,
            );
        }
    }
    c.header(
        "System schematic",
        &format!(
            "{} components · {} routed branches · captured model and current analysis layout",
            snapshot.cards.len(),
            snapshot.edges.len()
        ),
    );
    c.rect(
        0.,
        size.height as f32 - 24.,
        size.width as f32,
        24.,
        [247, 250, 252],
    );
    c.text(
        28.,
        size.height as f32 - 19.,
        12.,
        "Typed ports and shared net routing · presentation only · no physics advanced",
        MUTED,
        size.width as f32 - 56.,
    );
    let mut metadata = snapshot.metadata.clone();
    metadata["kind"] = json!("schematic");
    metadata["size"] = json!(size);
    metadata["fit"] = json!(options.fit);
    metadata["visible_components"] =
        json!(snapshot.cards.iter().map(|c| &c.id).collect::<Vec<_>>());
    Ok(Rendered {
        png: c.png()?,
        metadata,
    })
}
