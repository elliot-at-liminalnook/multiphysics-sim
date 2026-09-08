use nalgebra::{Matrix3, Vector3 as V};
use sim_domain_robot::articulated::{ContactPoint, Evaluation, LinkKin};

#[test]
fn loaded_material_velocity_includes_rotation_and_excludes_internal_contacts() {
    let mut e = Evaluation::default();
    e.links.push(LinkKin {r: Matrix3::identity(), p: V::zeros(), w: V::new(0.,0.,-0.1),
        vel: V::new(0.01,0.,2.), alpha: V::zeros(), acc: V::zeros()});
    let contact = |y, f| ContactPoint {link:0, other:None, point:V::new(0.,y,0.),
        force:V::new(0.,0.,f), penetration:0.001};
    e.contacts.push(contact(-0.1,1.));
    let (f,v) = e.world_z_floor_contact_velocity(0).unwrap();
    assert_eq!(f,1.); assert!(v.norm() < 1e-16); // rolling cancellation
    e.contacts.push(contact(0.1,3.));
    let mut internal = contact(10.,1000.); internal.other=Some(1); e.contacts.push(internal);
    let (f,v) = e.world_z_floor_contact_velocity(0).unwrap();
    assert_eq!(f,4.); assert!((v.x-0.015).abs() < 1e-16); assert_eq!(v.y,0.); assert_eq!(v.z,0.);
    assert!(e.world_z_floor_contact_velocity(1).is_err());
    e.contacts.clear(); assert_eq!(e.world_z_floor_contact_velocity(0).unwrap(), (0.,V::zeros()));
    e.contacts.push(contact(0.,-1.)); assert!(e.world_z_floor_contact_velocity(0).is_err());
}
