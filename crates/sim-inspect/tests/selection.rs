use sim_inspect::{SystemDescription, selection::SelectionTarget};
use std::collections::BTreeSet;
fn fixture() -> SystemDescription {
    serde_json::from_str(include_str!(
        "../../../examples/systems-viewer/spatial/motor-thermal.description.json"
    ))
    .unwrap()
}
#[test]
fn nets_retain_all_terminals_and_components_do_not_select_neighbors() {
    let d = fixture();
    for net in d.nets.values() {
        let h = SelectionTarget::net(&net.id).resolve(&d).unwrap();
        assert_eq!(h.nets, BTreeSet::from([net.id.clone()]));
        assert_eq!(h.ports, net.ports.iter().cloned().collect());
        assert_eq!(
            h.components,
            net.ports
                .iter()
                .map(|p| d.ports[p].component.clone())
                .collect()
        );
    }
    let id = "example/motor-thermal/motor";
    let h = SelectionTarget::component(id).resolve(&d).unwrap();
    assert_eq!(h.components, BTreeSet::from([id.into()]));
    assert!(!h.nets.is_empty());
    assert!(SelectionTarget::component("invented").validate(&d).is_err());
    assert!(
        SelectionTarget::Ports {
            ids: BTreeSet::new()
        }
        .validate(&d)
        .is_err()
    );
}

#[cfg(unix)]
mod native {
    use super::*;
    use sim_inspect::selection::native::{Peer, SelectionClient, create_session};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    fn until(mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while !check() {
            assert!(Instant::now() < deadline, "selection link did not converge");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn bidirectional_link_has_no_echo_reconnects_and_rejects_foreign_state() {
        let d = Arc::new(fixture());
        let dir = create_session(&d, SelectionTarget::None).unwrap();
        let mut a = SelectionClient::connect(
            d.clone(),
            dir.clone(),
            Peer::Assembly,
            SelectionTarget::None,
        )
        .unwrap();
        let mut b = SelectionClient::connect(
            d.clone(),
            dir.clone(),
            Peer::Schematic,
            SelectionTarget::None,
        )
        .unwrap();
        until(|| a.snapshot().peer_online && b.snapshot().peer_online);
        let motor = SelectionTarget::component("example/motor-thermal/motor");
        assert_eq!(a.exchange(motor.clone()).unwrap(), motor);
        let mut b_ui = SelectionTarget::None;
        until(|| {
            b_ui = b.exchange(b_ui.clone()).unwrap();
            b_ui == motor
        });
        let net = SelectionTarget::net(d.nets.keys().next().unwrap());
        b.exchange(net.clone()).unwrap();
        let mut a_ui = motor;
        until(|| {
            a_ui = a.exchange(a_ui.clone()).unwrap();
            a_ui == net
        });
        until(|| b.snapshot().acknowledged_request == 1);
        let seq = b.snapshot().record.unwrap().sequence;
        let mut b_ui = net.clone();
        for _ in 0..20 {
            a_ui = a.exchange(a_ui).unwrap();
            b_ui = b.exchange(b_ui).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            a.snapshot().record.unwrap().sequence,
            seq,
            "remote application must not echo"
        );
        assert_eq!(b_ui, net);
        // New local input wins over an already queued old remote snapshot.
        a.exchange(SelectionTarget::None).unwrap();
        let newest = SelectionTarget::component("example/motor-thermal/supply");
        assert_eq!(a.exchange(newest.clone()).unwrap(), newest);
        until(|| {
            b_ui = b.exchange(b_ui.clone()).unwrap();
            b_ui == newest
        });
        drop(b);
        until(|| !a.snapshot().peer_online);
        let mut b = SelectionClient::connect(
            d.clone(),
            dir.clone(),
            Peer::Schematic,
            SelectionTarget::None,
        )
        .unwrap();
        let mut b_ui = SelectionTarget::None;
        until(|| {
            b_ui = b.exchange(b_ui.clone()).unwrap();
            b_ui == newest && a.snapshot().peer_online
        });
        let duplicate = SelectionClient::connect(
            d.clone(),
            dir.clone(),
            Peer::Assembly,
            SelectionTarget::None,
        )
        .unwrap();
        until(|| duplicate.snapshot().error.is_some());
        drop(duplicate);
        let bytes = std::fs::read(dir.join("selection.json")).unwrap();
        let mut foreign: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        foreign["description_id"] = "foreign".into();
        std::fs::write(
            dir.join("foreign.tmp"),
            serde_json::to_vec(&foreign).unwrap(),
        )
        .unwrap();
        std::fs::rename(dir.join("foreign.tmp"), dir.join("selection.json")).unwrap();
        until(|| a.snapshot().error.is_some() && b.snapshot().error.is_some());
        assert_eq!(b.exchange(b_ui).unwrap(), newest);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(dir.join("selection.json")).unwrap()
            )
            .unwrap()["description_id"],
            "foreign"
        );
        drop(a);
        drop(b);
        // Wait for both OS leases to be relinquished before removing test artifacts.
        until(|| {
            ["assembly.lease", "schematic.lease"].iter().all(|name| {
                let f = std::fs::File::open(dir.join(name)).unwrap();
                f.try_lock().is_ok()
            })
        });
        std::fs::remove_dir_all(dir).unwrap();
    }
}
