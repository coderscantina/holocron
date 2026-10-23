//! Exact decimal arithmetic on JSON number text, so property typing and revenue rounding never
//! pass through a binary float (decision 0002, Property type system and Currencies).

/// A JSON number as `digits * 10^exponent`, without leading or trailing zeros in `digits`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimal {
    negative: bool,
    digits: Vec<u8>,
    exponent: i64,
}

impl Decimal {
    /// Parses JSON number grammar: `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`.
    pub fn parse(text: &str) -> Option<Self> {
        let (negative, rest) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (mantissa, exponent) = match rest.find(['e', 'E']) {
            Some(at) => (&rest[..at], rest[at + 1..].parse::<i64>().ok()?),
            None => (rest, 0),
        };
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if whole.is_empty()
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut digits: Vec<u8> = whole
            .bytes()
            .chain(fraction.bytes())
            .map(|b| b - b'0')
            .collect();
        let mut exponent = exponent.checked_sub(fraction.len() as i64)?;
        let leading = digits.iter().take_while(|d| **d == 0).count();
        digits.drain(..leading);
        while digits.last() == Some(&0) {
            digits.pop();
            exponent += 1;
        }
        Some(Self {
            negative,
            digits,
            exponent,
        })
    }

    pub fn is_zero(&self) -> bool {
        self.digits.is_empty()
    }

    /// True when the exact value has no fractional part.
    pub fn is_whole(&self) -> bool {
        self.is_zero() || self.exponent >= 0
    }

    /// The value times `10^scale`, rounded half to even. `None` beyond 38 significant digits.
    pub fn scaled(&self, scale: u32) -> Option<i128> {
        if self.is_zero() {
            return Some(0);
        }
        let shift = self.exponent + scale as i64;
        let magnitude = if shift >= 0 {
            if self.digits.len() as i64 + shift > 38 {
                return None;
            }
            let mut value = digits_value(&self.digits)?;
            for _ in 0..shift {
                value = value.checked_mul(10)?;
            }
            value
        } else {
            let dropped = (-shift) as usize;
            if dropped > self.digits.len() {
                // Less than one unit at the target scale and below one half: rounds to zero.
                0
            } else {
                let split = self.digits.len() - dropped;
                let mut kept = digits_value(&self.digits[..split])?;
                let first = self.digits[split];
                let rest = self.digits[split + 1..].iter().any(|d| *d != 0);
                if first > 5 || (first == 5 && (rest || kept % 2 == 1)) {
                    kept += 1;
                }
                kept
            }
        };
        Some(if self.negative { -magnitude } else { magnitude })
    }
}

fn digits_value(digits: &[u8]) -> Option<i128> {
    if digits.len() > 38 {
        return None;
    }
    Some(
        digits
            .iter()
            .fold(0i128, |value, d| value * 10 + *d as i128),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaled(text: &str, scale: u32) -> Option<i128> {
        Decimal::parse(text).unwrap().scaled(scale)
    }

    #[test]
    fn rounds_half_to_even_from_the_exact_text() {
        assert_eq!(scaled("19.995", 2), Some(2000));
        assert_eq!(scaled("0.12345", 4), Some(1234));
        assert_eq!(scaled("0.12355", 4), Some(1236));
        assert_eq!(scaled("-2.5", 0), Some(-2));
        assert_eq!(scaled("0.004", 2), Some(0));
        assert_eq!(scaled("1e3", 0), Some(1000));
        assert_eq!(scaled("1e400", 0), None);
    }

    #[test]
    fn whole_numbers_are_classified_by_value() {
        assert!(Decimal::parse("1.0").unwrap().is_whole());
        assert!(Decimal::parse("1e3").unwrap().is_whole());
        assert!(Decimal::parse("-0").unwrap().is_whole());
        assert!(!Decimal::parse("2.00001").unwrap().is_whole());
        assert!(!Decimal::parse("15e-1").unwrap().is_whole());
    }
}
