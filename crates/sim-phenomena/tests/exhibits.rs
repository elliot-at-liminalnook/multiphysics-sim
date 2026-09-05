//! Every exhibit must build, run in real time, respond to its knob and reset.

use sim_phenomena::exhibits::all;
use std::time::Instant;

#[test]
fn every_exhibit_runs_in_real_time() {
    let mut slow = Vec::new();
    for mut exhibit in all() {
        let title = exhibit.title();
        let knob = exhibit.knob();
        assert!(knob.min <= knob.value && knob.value <= knob.max, "{title}: knob out of range");
        // Two simulated seconds of real time at the exhibit's own scale.
        let budget = 2.0 * exhibit.time_scale();
        let started = Instant::now();
        exhibit.advance(budget).unwrap_or_else(|e| panic!("{title}: {e}"));
        let elapsed = started.elapsed().as_secs_f64();
        if elapsed > 1.0 {
            slow.push(format!("{title}: {elapsed:.2} s for 2 s of real time"));
        }
        assert!(exhibit.time() > 0.0, "{title}: time did not advance");
        let mut shapes = Vec::new();
        exhibit.shapes(&mut shapes);
        assert!(!shapes.is_empty(), "{title}: no shapes");
        assert!(!exhibit.readouts().is_empty(), "{title}: no readouts");
        assert!(exhibit.signal().1.is_finite(), "{title}: non-finite signal");
        assert!(!exhibit.verdict().is_empty(), "{title}: empty verdict");

        // The knob rebuilds; reset returns to the start.
        exhibit.set_knob(knob.min);
        assert_eq!(exhibit.knob().value, knob.min, "{title}: knob not applied");
        exhibit.advance(0.5 * exhibit.time_scale()).unwrap_or_else(|e| panic!("{title} at knob min: {e}"));
        exhibit.set_knob(knob.max);
        exhibit.advance(0.5 * exhibit.time_scale()).unwrap_or_else(|e| panic!("{title} at knob max: {e}"));
        exhibit.reset();
        assert!(exhibit.time().abs() < 1.0e-9, "{title}: reset did not rewind time");
    }
    assert!(slow.is_empty(), "exhibits slower than real time:\n{}", slow.join("\n"));
}
