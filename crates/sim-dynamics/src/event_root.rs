//! Shared bracketed event location, independent of the state's representation
//! and of the integration method used to evaluate intermediate times.
#[derive(Clone, Copy, Debug)]
pub struct CrossingBracket {
    pub duration: f64,
    pub relative_tolerance: f64,
    pub before: f64,
    pub after: f64,
}

#[derive(Debug)]
pub enum RootError<E> {
    InvalidBracket,
    NonFiniteGuard,
    Evaluation(E),
}

/// Locate a nonnegative-to-negative guard crossing. Every `evaluate(dt)` must
/// advance from the SAME unmodified beginning state and return that guard.
/// Returns a time just past the crossing, within the declared time/value band.
/// It does not commit a state or apply a jump. Endpoint sign tests cannot detect
/// multiple hidden crossings; integration-step refinement remains necessary.
pub fn locate_crossing<E>(
    bracket: CrossingBracket,
    mut evaluate: impl FnMut(f64) -> Result<f64, E>,
) -> Result<f64, RootError<E>> {
    let CrossingBracket {
        duration: h,
        relative_tolerance: tolerance,
        before,
        after,
    } = bracket;
    if !h.is_finite()
        || h <= 0.0
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || tolerance >= 1.0
        || !before.is_finite()
        || before < 0.0
        || !after.is_finite()
        || after >= 0.0
    {
        return Err(RootError::InvalidBracket);
    }
    let epsilon = h * 0.5_f64.powi((1.0 / tolerance).log2().ceil() as i32);
    if epsilon == 0.0 {
        return Err(RootError::InvalidBracket);
    }
    let (mut low, mut high) = (0.0, h);
    let (mut f_low, mut f_high) = (before, after);
    if f_low == 0.0 {
        return Ok(epsilon);
    }
    let band = tolerance * f_low.abs().max(f_high.abs());
    let mut side = 0i8;
    let mut slow = 0u8;
    while high - low > tolerance * h {
        let width = high - low;
        let secant = if f_low > 0.0 && f_high < 0.0 {
            low + width * f_low / (f_low - f_high)
        } else {
            low + 0.5 * width
        };
        let (inner_low, inner_high) = (low + epsilon, high - epsilon);
        let mid = if slow >= 2 || inner_low >= inner_high {
            low + 0.5 * width
        } else {
            secant.clamp(inner_low, inner_high)
        };
        let f = evaluate(mid).map_err(RootError::Evaluation)?;
        if !f.is_finite() {
            return Err(RootError::NonFiniteGuard);
        }
        if f.abs() <= band || (f < 0.0 && mid - low <= tolerance * h) {
            return Ok(if f < 0.0 { mid } else { (mid + epsilon).min(h) });
        }
        if f >= 0.0 && high - mid <= tolerance * h {
            return Ok(high);
        }
        slow = if (if f >= 0.0 { high - mid } else { mid - low }) > 0.5 * width {
            slow + 1
        } else {
            0
        };
        if f >= 0.0 {
            low = mid;
            f_low = f;
            if side == 1 {
                f_high *= 0.5;
            }
            side = 1;
        } else {
            high = mid;
            f_high = f;
            if side == -1 {
                f_low *= 0.5;
            }
            side = -1;
        }
    }
    Ok(high)
}
