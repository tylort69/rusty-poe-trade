//! Port of upstream `filters/util.ts`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundMethod {
    Floor,
    Ceil,
}

impl RoundMethod {
    fn apply(self, v: f64) -> f64 {
        match self {
            RoundMethod::Floor => v.floor(),
            RoundMethod::Ceil => v.ceil(),
        }
    }
}

fn decimal_places(value: f64, dp: bool) -> i32 {
    if !dp || value.abs() >= 10.0 {
        0
    } else if value.abs() < 2.3 {
        2
    } else {
        1
    }
}

pub fn round_roll(value: f64, dp: bool) -> f64 {
    let rounding = 10f64.powi(decimal_places(value, dp));
    (value * rounding).trunc() / rounding
}

pub fn percent_roll(value: f64, p: f64, method: RoundMethod, dp: bool) -> f64 {
    let res = value + value.abs() * p / 100.0;
    let rounding = 10f64.powi(decimal_places(value, dp));
    method.apply((res + f64::EPSILON) * rounding) / rounding
}

pub fn percent_roll_delta(value: f64, delta: f64, p: f64, method: RoundMethod, dp: bool) -> f64 {
    let res = value + delta * p / 100.0;
    let rounding = 10f64.powi(decimal_places(value, dp));
    method.apply((res + f64::EPSILON) * rounding) / rounding
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls() {
        assert_eq!(percent_roll(100.0, -10.0, RoundMethod::Floor, false), 90.0);
        assert_eq!(percent_roll(100.0, 10.0, RoundMethod::Ceil, false), 110.0);
        assert_eq!(percent_roll(-50.0, -10.0, RoundMethod::Floor, false), -55.0);
        assert_eq!(round_roll(1.234, true), 1.23);
        assert_eq!(round_roll(5.55, true), 5.5);
        assert_eq!(percent_roll_delta(30.0, 10.0, -10.0, RoundMethod::Floor, false), 29.0);
    }
}
