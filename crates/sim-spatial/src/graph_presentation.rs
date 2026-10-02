//! Reusable native schematic drawing, shared by Build and CAD. Inputs are
//! display routes and a view transform; this module cannot mutate source data.
use bevy::prelude::*;
pub(crate) fn route(
    canvas: &mut ChildSpawnerCommands,
    route: &sim_diagram::NetRoute,
    at: impl Fn(sim_inspect::Point) -> Vec2,
    color: Color,
    thick: f32,
) {
    for branch in &route.branches {
        for pair in branch.windows(2) {
            let (a, b) = (at(pair[0]), at(pair[1]));
            for (p, q) in [(a, Vec2::new(b.x, a.y)), (Vec2::new(b.x, a.y), b)] {
                if p == q {
                    continue;
                }
                let lo = p.min(q);
                canvas.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(lo.x - (thick / 2.).floor()),
                        top: Val::Px(lo.y - (thick / 2.).floor()),
                        width: Val::Px((p.x - q.x).abs().max(thick)),
                        height: Val::Px((p.y - q.y).abs().max(thick)),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
            }
        }
    }
    for junction in &route.junctions {
        let p = at(*junction);
        canvas.spawn((
            Node {
                border_radius: BorderRadius::all(Val::Px(3.)),
                position_type: PositionType::Absolute,
                left: Val::Px(p.x - 3.),
                top: Val::Px(p.y - 3.),
                width: Val::Px(6.),
                height: Val::Px(6.),
                ..default()
            },
            BackgroundColor(color),
        ));
    }
}
