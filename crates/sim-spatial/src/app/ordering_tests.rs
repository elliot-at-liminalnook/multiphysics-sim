//! Build the public ordering graph without any window, assets or GPU.
use super::{InputSet, ViewerSet};
use crate::camera::CameraSet;
use crate::inspect_view::InspectViewSet;
use bevy::prelude::*;

#[test]
fn public_sets_and_spatial_consumers_have_no_ordering_cycle() {
    let mut app = App::new();
    super::configure_sets(&mut app);
    crate::camera::configure_sets(&mut app);
    crate::robot::configure_sets(&mut app);
    crate::cad::configure_sets(&mut app);
    crate::inspect_view::configure_sets(&mut app);
    // Materialize each public CAD and robot node so schedule construction
    // exercises the configured subset hierarchy and key precedence too.
    app.add_systems(Update, (
        (|| {}).in_set(crate::cad::CadSet::Results),
        (|| {}).in_set(crate::cad::CadSet::Plane),
        (|| {}).in_set(crate::cad::CadSet::View).after(CameraSet::Place),
        (|| {}).in_set(crate::cad::CadKeySet::Gate),
        (|| {}).in_set(crate::cad::CadKeySet::Focus),
        (|| {}).in_set(crate::cad::CadKeySet::Keys),
        (|| {}).in_set(crate::cad::CadKeySet::ToolKeys),
        (|| {}).in_set(crate::robot::RobotSet::Actions),
        (|| {}).in_set(CameraSet::Viewport),
        (|| {}).in_set(CameraSet::Navigate),
        (|| {}).in_set(CameraSet::Place),
    ));
    app.add_systems(Update, (
        (|| {}).in_set(crate::cad::CadSet::Mesh),
        (|| {}).in_set(crate::cad::CadSet::Highlight),
        || {},
    ).chain().before(CameraSet::Place).in_set(ViewerSet::SimSync));
    app.add_systems(Update, (
        || {}, || {}, || {},
        (|| {}).in_set(crate::robot::RobotSet::Frames),
        || {}, || {}, || {}, || {},
    ).chain().before(CameraSet::Viewport).in_set(ViewerSet::SimSync));
    app.add_systems(Update, (
        (|| {}).after(crate::robot::RobotSet::Frames),
        (|| {}).before(crate::robot::RobotSet::Frames),
        (|| {}).after(crate::cad::CadSet::Mesh),
        (|| {}).after(crate::cad::CadSet::Highlight),
    ).in_set(ViewerSet::SimSync));
    app.add_systems(Update, (|| {}).after(crate::robot::RobotSet::Actions).in_set(ViewerSet::Actions));
    app.add_systems(Update, (|| {}).after(crate::cad::CadSet::Results).in_set(ViewerSet::JobResults));
    // No-op systems mirror the spatial plugin's two chains, including
    // singleton public ordering points. Their resources are irrelevant to
    // schedule construction; the edges and membership are the same.
    app.add_systems(Update, (
        (|| {}).in_set(InspectViewSet::Notes),
        || {},
        (|| {}).in_set(InspectViewSet::Link),
        || {},
        || {},
        (|| {}).in_set(InspectViewSet::Camera),
    ).chain().before(CameraSet::Viewport).in_set(ViewerSet::SimSync));
    app.add_systems(Update, (
        || {},
        (|| {}).in_set(InspectViewSet::Parts),
        || {}, || {}, || {}, || {}, || {},
    ).chain().after(CameraSet::Place).in_set(ViewerSet::SimSync));
    // Selection projection, builder frame, placed-camera readers, and
    // lesson frame keep their original consumers' constraints.
    app.add_systems(Update, (
        (|| {}).after(InspectViewSet::Notes).before(InspectViewSet::Link).before(InspectViewSet::Parts),
        (|| {}).before(InspectViewSet::Camera).before(InspectViewSet::Parts),
        (|| {}).after(InspectViewSet::Parts).after(CameraSet::Place),
        (|| {}).after(InspectViewSet::Notes),
        (|| {}).before(InspectViewSet::Camera),
    ).in_set(ViewerSet::SimSync));
    app.add_systems(Update, (|| {}).in_set(InputSet::Rest));
    app.add_systems(Update, (|| {}).in_set(InputSet::Window));
    app.world_mut().schedule_scope(Update, |world, schedule| {
        schedule.initialize(world).expect("public Update sets must form an acyclic schedule");
    });
}

#[test]
fn picker_and_kit_text_ordering_has_no_cycle() {
    let mut app = App::new();
    crate::ui_kit::text::configure_sets(&mut app);
    app.add_systems(PreUpdate, (
        (|| {}).before(crate::ui_kit::text::TextInputSet),
        (|| {}).in_set(crate::ui_kit::text::TextInputSet),
        (|| {}).after(bevy::input::InputSystems).after(crate::ui_kit::text::TextInputSet),
    ));
    app.world_mut().schedule_scope(PreUpdate, |world, schedule| {
        schedule.initialize(world).expect("picker and kit text sets must form an acyclic schedule");
    });
}
