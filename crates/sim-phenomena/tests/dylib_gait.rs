//! The in-process (shared library) gait drives the quadruped like the Python one.
use sim_phenomena::scenarios::quadruped_gait::{Lang, Quadruped};
use sim_phenomena::world::registry;

#[test]
fn the_dylib_gait_stands_and_steps() {
    let registry = registry();
    let q = Quadruped { start: 0.2, ..Quadruped::default() };
    let mut plant = q.model(&registry);
    plant.runtime.attach(plant.seam, q.controller_in(q.stride, Lang::Dylib).unwrap()).unwrap();
    plant.runtime.advance(0.6, 1.0e-3).unwrap();
    let height = plant.runtime.get(plant.body[1]);
    assert!(height > 0.4 && height < 0.6, "height {height}");
}
