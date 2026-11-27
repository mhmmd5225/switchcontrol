/// Convert 120 wheel units per step to Quartz's default ten points per line.
/// Preserve the fractional remainder and accumulate in widened integers.
pub(crate) fn to_points(units: i32, remainder: &mut i32) -> i32 {
    let total = i64::from(units) + i64::from(*remainder);
    *remainder = (total % 12) as i32;
    (total / 12) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_scrolls_are_delivered_before_a_full_notch() {
        let mut remainder = 0;
        assert_eq!(to_points(5, &mut remainder), 0);
        assert_eq!(to_points(7, &mut remainder), 1);
        assert_eq!(to_points(36, &mut remainder), 3);
        assert_eq!(to_points(-36, &mut remainder), -3);
        assert_eq!(remainder, 0);
    }
}
