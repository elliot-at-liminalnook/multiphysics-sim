mod common;
use common::*;
use sim_core::ModelWorld;
use sim_domain_robot::switchable_bridge::SWITCHABLE_H_BRIDGE;

#[test]
fn disabled_bridge_has_passive_leakage_and_bidirectional_supply_clamps() {
    for voltage in [-15_f64, -6., 0., 6., 15.] {
        let registry = registry();
        let mut m = ModelWorld::default();
        let supply = m
            .part(
                &registry,
                "supply",
                sim_domain_electrical::elements::VOLTAGE_SOURCE,
                [("voltage", 12.)],
            )
            .unwrap();
        let terminal = m
            .part(
                &registry,
                "terminal",
                sim_domain_electrical::elements::VOLTAGE_SOURCE,
                [("voltage", voltage)],
            )
            .unwrap();
        let ground = m
            .part(
                &registry,
                "ground",
                sim_domain_electrical::elements::GROUND,
                [],
            )
            .unwrap();
        let bridge = m
            .part(
                &registry,
                "bridge",
                SWITCHABLE_H_BRIDGE,
                [
                    ("off_conductance", 1e-6),
                    ("diode_drop", 0.5),
                    ("diode_resistance", 1.),
                ],
            )
            .unwrap();
        let command = m
            .part(
                &registry,
                "command",
                sim_domain_control::elements::CONSTANT,
                [("value", 1.)],
            )
            .unwrap();
        let off = m
            .part(
                &registry,
                "off",
                sim_domain_control::elements::CONSTANT,
                [("value", 0.)],
            )
            .unwrap();
        m.connect([supply.port("p"), bridge.port("supply_p")]);
        m.connect([
            supply.port("n"),
            bridge.port("supply_n"),
            bridge.port("n"),
            terminal.port("n"),
            ground.port("pin"),
        ]);
        m.connect([terminal.port("p"), bridge.port("p")]);
        m.connect([command.port("value"), bridge.port("command")]);
        m.connect([off.port("value"), bridge.port("enabled")]);
        let mut rt = sim_compile::Runtime::new(m, &registry, euler()).unwrap();
        rt.advance(0.01, 0.001).unwrap();
        let current = rt.get(rt.state_id(bridge.behavior, "current"));
        let regenerated = rt.get(rt.state_id(supply.behavior, "current"));
        let diode = (voltage.abs() - 13.).max(0.);
        assert!((current - (voltage * 1e-6 + voltage.signum() * diode)).abs() < 1e-8);
        assert!((regenerated - diode).abs() < 1e-8);
        let loss = voltage * current - 12. * regenerated;
        assert!(
            loss >= -1e-8,
            "Disabled bridge must not create electrical energy"
        );
        assert!((loss - (1e-6 * voltage * voltage + diode * (1. + diode))).abs() < 1e-8);
    }
}
