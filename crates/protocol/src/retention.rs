//! How long analytical events are kept (decision 0016). Replays and heatmaps keep their own
//! limits (0006).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A retention period. Ordered from shortest to longest, so `a < b` means `a` keeps less.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    TS,
    JsonSchema,
)]
pub enum Retention {
    #[default]
    #[serde(rename = "1y")]
    OneYear,
    #[serde(rename = "2y")]
    TwoYears,
    #[serde(rename = "3y")]
    ThreeYears,
    #[serde(rename = "5y")]
    FiveYears,
    #[serde(rename = "10y")]
    TenYears,
    #[serde(rename = "forever")]
    Forever,
}

impl Retention {
    pub const ALL: [Retention; 6] = [
        Retention::OneYear,
        Retention::TwoYears,
        Retention::ThreeYears,
        Retention::FiveYears,
        Retention::TenYears,
        Retention::Forever,
    ];

    /// Calendar months kept before the current day, `None` for [`Retention::Forever`].
    pub const fn months(self) -> Option<u32> {
        match self {
            Retention::OneYear => Some(12),
            Retention::TwoYears => Some(24),
            Retention::ThreeYears => Some(36),
            Retention::FiveYears => Some(60),
            Retention::TenYears => Some(120),
            Retention::Forever => None,
        }
    }

    /// The wire and storage code, for example `5y`.
    pub const fn code(self) -> &'static str {
        match self {
            Retention::OneYear => "1y",
            Retention::TwoYears => "2y",
            Retention::ThreeYears => "3y",
            Retention::FiveYears => "5y",
            Retention::TenYears => "10y",
            Retention::Forever => "forever",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.code() == code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_on_the_wire() {
        for retention in Retention::ALL {
            let wire = serde_json::to_value(retention).unwrap();
            assert_eq!(wire, retention.code());
            assert_eq!(Retention::from_code(retention.code()), Some(retention));
        }
        assert!(Retention::OneYear < Retention::Forever);
    }
}
