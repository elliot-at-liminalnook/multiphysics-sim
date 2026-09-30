//! Integer PD plus velocity feedforward, shared by Rust and synthesized RTL.
//! Positions and per-tick displacements are encoder counts; output is PWM counts.
//! This is a discrete-time controller: acquisition and command times belong in the recording.
//!
//! The controller implementation lives in `fixed_pd/law.rs`: gains and their
//! validation, the expression graph, `step` and the generated RTL. Its bytes are
//! the implementation identity that CAD actuator profiles and controller
//! records carry, so helpers added here do not invalidate measured scenes while
//! any edit to the law does.
mod law;
pub use law::{Gains, step, verilog};

/// Exact source of the fixed-PD controller implementation.
pub const IMPLEMENTATION_SOURCE: &[u8] = include_bytes!("fixed_pd/law.rs");

/// Identity of the given controller implementation source (blake3, hex).
pub fn identity_of(source: &[u8]) -> String {
    blake3::hash(source).to_hex().to_string()
}

/// Identity of the compiled fixed-PD controller implementation. The single
/// definition behind `implementation_blake3` checks and `controller_ir` records.
pub fn implementation_identity() -> String {
    identity_of(IMPLEMENTATION_SOURCE)
}

/// The same law and rounding on count differences, for a multi-turn encoder
/// whose absolute count no longer fits 12 bits. `error` = target − position,
/// `moved` = position − previous, `delta` = target step; each is bounded like
/// the single-turn inputs, so every intermediate still fits signed 32 bits.
pub fn step_differences(g: Gains, error: i32, moved: i32, delta: i32) -> Result<i16, String> {
    g.validate()?;
    if [error, moved, delta].iter().any(|x| x.unsigned_abs() > 4095) {
        return Err("Fixed PD differences must stay within 4095 encoder counts".into());
    }
    let raw = law::law().eval(&[
        error,
        0,
        -moved,
        delta,
        g.kp_q8.into(),
        g.kd_q8.into(),
        g.kv_q8.into(),
    ]);
    Ok((raw / 256).clamp(-(g.limit as i32), g.limit as i32) as i16)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_quantization_and_saturation() {
        let g = Gains {
            kp_q8: 256,
            kd_q8: 0,
            kv_q8: 0,
            limit: 50,
        };
        assert_eq!(step(g, 2001, 2000, 2000, 0).unwrap(), 1);
        assert_eq!(step(g, 1900, 2000, 2000, 0).unwrap(), -50);
        assert_eq!(
            step(Gains { kp_q8: 1, ..g }, 1999, 2000, 2000, 0).unwrap(),
            0
        );
        assert!(step(g, 4096, 2000, 2000, 0).is_err());
    }

    #[test]
    fn implementation_identity_covers_the_law_only() {
        let law = std::str::from_utf8(IMPLEMENTATION_SOURCE).unwrap();
        // The identity is the law file: this module's helpers are outside it.
        assert!(law.contains("pub fn step(") && law.contains("pub fn verilog()"));
        assert!(!law.contains("step_differences"));
        let whole = include_bytes!("fixed_pd.rs");
        assert_ne!(implementation_identity(), identity_of(whole));
        let mut extended = whole.to_vec();
        extended.extend_from_slice(b"\npub fn unrelated_helper() {}\n");
        assert_ne!(identity_of(whole), identity_of(&extended));
        assert_eq!(implementation_identity(), identity_of(IMPLEMENTATION_SOURCE));
        // Any edit to the law, here its rounding, changes the identity.
        let changed = law.replacen("Ok((raw / 256)", "Ok((raw / 128)", 1);
        assert_ne!(changed, law);
        assert_ne!(identity_of(changed.as_bytes()), implementation_identity());
        // Measured example scenes store this value (migrated from ae6b0dd1's
        // whole-file hash); changing the law requires re-exporting them.
        assert_eq!(
            implementation_identity(),
            "f50894e2a6d8c86b64e08f65ae8739284ef486fc3c8a89eb6492e4b66e1eb26d"
        );
    }
}
