use std::io::prelude::*;

use crate::ulp::Ulp;

/// Iterator over all possible f32 values.
pub struct AllF32s {
    next: u32,
}

impl AllF32s {
    pub fn new() -> AllF32s {
        AllF32s { next: 0 }
    }
}

impl Iterator for AllF32s {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.next == u32::MAX {
            None
        } else {
            let next = f32::from_bits(self.next);
            self.next += 1;
            Some(next)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (u32::MAX - self.next) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for AllF32s {}

/// Iterator that wrapper an inner iterator and logs progress messages as
/// items are pulled from it.
pub struct Progress<I: Iterator> {
    prefix: String,
    inner: I,
    remaining: usize,
    len: usize,
    report_step: usize,
}

impl<'a, I: Iterator> Progress<I> {
    /// Wrap the iterator `inner` with an iterator that prints progress messages
    /// prefixed by `prefix`.
    pub fn wrap(inner: I, prefix: &str) -> Progress<I> {
        let remaining = inner.size_hint().0;
        let report_step = (remaining / 1000).max(1);
        Progress {
            inner,
            remaining,
            len: remaining,
            report_step,
            prefix: prefix.to_string(),
        }
    }
}

impl<I: Iterator> Iterator for Progress<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining % self.report_step == 0 {
            let done = self.len - self.remaining;
            let progress = done as f32 / self.len as f32;
            print!("\r{}: {:.2}%", self.prefix, progress * 100.);
            let _ = std::io::stdout().flush();
        } else if self.remaining == 0 {
            println!("");
        }
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

/// Iterator over an arithmetic range. See [arange].
pub struct ARange<T: Copy + PartialOrd + std::ops::Add<Output = T>> {
    next: T,
    end: T,
    step: T,
}

/// Return an iterator over an arithmetic range `[start, end)` in steps of `step`.
///
/// Iteration stops if the next value in the series cannot be compared against
/// the end value (ie. if `next.partial_cmp(end)` yields `None`).
pub fn arange<T: Copy + PartialOrd + std::ops::Add<Output = T>>(
    start: T,
    end: T,
    step: T,
) -> ARange<T> {
    ARange {
        next: start,
        end,
        step,
    }
}

impl<T: Copy + PartialOrd + std::ops::Add<Output = T>> Iterator for ARange<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        use std::cmp::Ordering;
        let next = self.next;
        match next.partial_cmp(&self.end) {
            Some(Ordering::Less) => {
                self.next = self.next + self.step;
                Some(next)
            }
            _ => None,
        }
    }
}

/// Compare results of an f32 operation from an iterator against
/// reference results.
///
/// `results` is an iterator yielding tuples of `(input, actual, expected)`
/// values.
///
/// `ulp_threshold` specifies the maximum allowed difference between
/// `actual` and `expected` in ULPs.
pub fn check_f32s_are_equal<I: Iterator<Item = (f32, f32, f32)>>(results: I, ulp_threshold: f32) {
    let mut max_diff_ulps = 0.0f32;
    let mut max_diff_x = 0.0f32;
    let mut max_diff_actual = 0.0f32;
    let mut max_diff_expected = 0.0f32;

    for (x, actual, expected) in results {
        if actual == expected {
            // Fast path for expected common case where results are exactly
            // equal.
            continue;
        }

        assert_eq!(
            expected.is_nan(),
            actual.is_nan(),
            "NaN mismatch at {x}. Actual {x} Expected {x}"
        );
        assert_eq!(
            expected.is_infinite(),
            actual.is_infinite(),
            "Infinite mismatch at {x}. Actual {actual} Expected {expected}"
        );

        if !expected.is_infinite() && !expected.is_nan() {
            let diff = (actual - expected).abs();
            let diff_ulps = diff / expected.ulp();
            if diff_ulps > max_diff_ulps {
                max_diff_ulps = max_diff_ulps.max(diff_ulps);
                max_diff_x = x;
                max_diff_actual = actual;
                max_diff_expected = expected;
            }
        }
    }
    assert!(
        max_diff_ulps <= ulp_threshold,
        "max diff against reference is {} ULPs for x = {}, actual = {}, expected = {}, ULP = {}. Above ULP threshold {}",
        max_diff_ulps,
        max_diff_x,
        max_diff_actual,
        max_diff_expected,
        max_diff_expected.ulp(),
        ulp_threshold
    );
}

/// Test a unary function against all possible values of a 32-bit float.
///
/// `op` is a function that takes an f32 and computes the actual and
/// expected values of the function, where the expected value is computed
/// using a reference implementation.
///
/// `ulp_threshold` specifies the maximum difference between the actual
/// and expected values, in ULPs, when the expected value is not infinite
/// or NaN.
pub fn check_with_all_f32s<F: Fn(f32) -> (f32, f32)>(
    op: F,
    ulp_threshold: f32,
    progress_msg: &str,
) {
    let actual_expected = AllF32s::new().map(|x| {
        let (actual, expected) = op(x);
        (x, actual, expected)
    });
    check_f32s_are_equal(Progress::wrap(actual_expected, progress_msg), ulp_threshold);
}