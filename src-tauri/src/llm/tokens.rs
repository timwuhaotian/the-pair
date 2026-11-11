//! The output cap a reply may ask for.
//!
//! The cap only ever shrinks what a route resolved, so the limit is floored at
//! one token and capped at the largest any provider accepts: one that is too
//! small loses work, one that is too large is rejected outright.

/// The largest output any provider accepts.
pub(super) const MAX_OUTPUT_TOKENS: u64 = 131_072;

/// The output cap for one reply: the limit the route resolved, floored at one
/// token and capped at [`MAX_OUTPUT_TOKENS`].
pub(super) fn cap_output(max: u64) -> u64 {
    max.clamp(1, MAX_OUTPUT_TOKENS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_cap_never_reaches_zero_and_has_a_ceiling() {
        assert_eq!(cap_output(0), 1);
        assert_eq!(cap_output(8_000), 8_000);
        assert_eq!(cap_output(u64::MAX), MAX_OUTPUT_TOKENS);
    }
}
