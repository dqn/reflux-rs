use serde::{Deserialize, Serialize};
use strum::{FromRepr, IntoStaticStr};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    Default,
    FromRepr,
    IntoStaticStr,
)]
#[repr(u8)]
pub enum Grade {
    #[default]
    #[strum(serialize = "-")]
    NoPlay = 0,
    F = 1,
    E = 2,
    D = 3,
    C = 4,
    B = 5,
    A = 6,
    #[strum(serialize = "AA")]
    Aa = 7,
    #[strum(serialize = "AAA")]
    Aaa = 8,
}

impl Grade {
    pub fn from_u8(value: u8) -> Option<Self> {
        Self::from_repr(value)
    }

    pub fn from_score_ratio(ratio: f64) -> Self {
        if ratio >= 8.0 / 9.0 {
            Self::Aaa
        } else if ratio >= 7.0 / 9.0 {
            Self::Aa
        } else if ratio >= 6.0 / 9.0 {
            Self::A
        } else if ratio >= 5.0 / 9.0 {
            Self::B
        } else if ratio >= 4.0 / 9.0 {
            Self::C
        } else if ratio >= 3.0 / 9.0 {
            Self::D
        } else if ratio >= 2.0 / 9.0 {
            Self::E
        } else {
            Self::F
        }
    }

    pub fn short_name(&self) -> &'static str {
        self.into()
    }
}

impl std::fmt::Display for Grade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grade_from_score_ratio() {
        assert_eq!(Grade::from_score_ratio(1.0), Grade::Aaa);
        assert_eq!(Grade::from_score_ratio(0.9), Grade::Aaa);
        assert_eq!(Grade::from_score_ratio(8.0 / 9.0), Grade::Aaa);
        assert_eq!(Grade::from_score_ratio(7.0 / 9.0), Grade::Aa);
        assert_eq!(Grade::from_score_ratio(6.0 / 9.0), Grade::A);
        assert_eq!(Grade::from_score_ratio(5.0 / 9.0), Grade::B);
        assert_eq!(Grade::from_score_ratio(4.0 / 9.0), Grade::C);
        assert_eq!(Grade::from_score_ratio(3.0 / 9.0), Grade::D);
        assert_eq!(Grade::from_score_ratio(2.0 / 9.0), Grade::E);
        assert_eq!(Grade::from_score_ratio(0.1), Grade::F);
    }
}
