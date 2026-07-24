//! Freshness normalization per provider.

use crate::research::types::WebError;
use serde::{Deserialize, Serialize};

/// Normalized freshness values at the MCP boundary.
///
/// Each provider adapter translates these to its own parameter format.
/// This follows the Cockburn principle: the port defines the canonical model,
/// adapters translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Freshness {
    Day,
    Week,
    Month,
    Year,
}

impl std::str::FromStr for Freshness {
    type Err = WebError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "day" | "d" | "1d" | "past_day" | "past day" | "24h" => Ok(Freshness::Day),
            "week" | "w" | "1w" | "past_week" | "past week" | "7d" | "pw" => Ok(Freshness::Week),
            "month" | "m" | "1m" | "past_month" | "past month" | "30d" | "pm" => {
                Ok(Freshness::Month)
            }
            "year" | "y" | "1y" | "past_year" | "past year" | "365d" | "py" => Ok(Freshness::Year),
            _ => Err(WebError::BadArgs(format!(
                "Unknown freshness: {s}. Use: day, week, month, year"
            ))),
        }
    }
}

impl std::fmt::Display for Freshness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Freshness::Day => write!(f, "day"),
            Freshness::Week => write!(f, "week"),
            Freshness::Month => write!(f, "month"),
            Freshness::Year => write!(f, "year"),
        }
    }
}

/// Returns provider-generic key-value pairs for the given freshness value.
///
/// Each provider translates normalized freshness into its own parameter format:
/// - Brave: `freshness=pw` (past week)
/// - Tavily: `days=7`
/// - SerpAPI: `tbs=qdr:w`
pub fn normalize_freshness(freshness: &Freshness) -> Vec<(&'static str, String)> {
    match freshness {
        Freshness::Day => vec![("days", "1".to_string())],
        Freshness::Week => vec![("days", "7".to_string())],
        Freshness::Month => vec![("days", "30".to_string())],
        Freshness::Year => vec![("days", "365".to_string())],
    }
}

/// Map freshness to Brave's parameter format.
pub fn freshness_brave(freshness: &Freshness) -> String {
    match freshness {
        Freshness::Day => "pd".to_string(),
        Freshness::Week => "pw".to_string(),
        Freshness::Month => "pm".to_string(),
        Freshness::Year => "py".to_string(),
    }
}

/// Map freshness to SerpAPI's `tbs` parameter format.
pub fn freshness_serpapi(freshness: &Freshness) -> String {
    match freshness {
        Freshness::Day => "qdr:d".to_string(),
        Freshness::Week => "qdr:w".to_string(),
        Freshness::Month => "qdr:m".to_string(),
        Freshness::Year => "qdr:y".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_parsing_known_values() {
        assert_eq!("day".parse::<Freshness>().unwrap(), Freshness::Day);
        assert_eq!("week".parse::<Freshness>().unwrap(), Freshness::Week);
        assert_eq!("month".parse::<Freshness>().unwrap(), Freshness::Month);
        assert_eq!("year".parse::<Freshness>().unwrap(), Freshness::Year);
    }

    #[test]
    fn freshness_parsing_aliases() {
        assert_eq!("d".parse::<Freshness>().unwrap(), Freshness::Day);
        assert_eq!("1d".parse::<Freshness>().unwrap(), Freshness::Day);
        assert_eq!("24h".parse::<Freshness>().unwrap(), Freshness::Day);
        assert_eq!("pw".parse::<Freshness>().unwrap(), Freshness::Week);
        assert_eq!("7d".parse::<Freshness>().unwrap(), Freshness::Week);
        assert_eq!("pm".parse::<Freshness>().unwrap(), Freshness::Month);
        assert_eq!("30d".parse::<Freshness>().unwrap(), Freshness::Month);
        assert_eq!("py".parse::<Freshness>().unwrap(), Freshness::Year);
        assert_eq!("365d".parse::<Freshness>().unwrap(), Freshness::Year);
    }

    #[test]
    fn freshness_parsing_unknown_is_error() {
        assert!("decade".parse::<Freshness>().is_err());
        assert!("century".parse::<Freshness>().is_err());
        assert!("".parse::<Freshness>().is_err());
    }

    #[test]
    fn normalize_freshness_days() {
        let day = normalize_freshness(&Freshness::Day);
        assert_eq!(day[0].1, "1");
        let week = normalize_freshness(&Freshness::Week);
        assert_eq!(week[0].1, "7");
        let month = normalize_freshness(&Freshness::Month);
        assert_eq!(month[0].1, "30");
        let year = normalize_freshness(&Freshness::Year);
        assert_eq!(year[0].1, "365");
    }

    #[test]
    fn freshness_brave_mapping() {
        assert_eq!(freshness_brave(&Freshness::Day), "pd");
        assert_eq!(freshness_brave(&Freshness::Week), "pw");
        assert_eq!(freshness_brave(&Freshness::Month), "pm");
        assert_eq!(freshness_brave(&Freshness::Year), "py");
    }

    #[test]
    fn freshness_serpapi_mapping() {
        assert_eq!(freshness_serpapi(&Freshness::Day), "qdr:d");
        assert_eq!(freshness_serpapi(&Freshness::Week), "qdr:w");
        assert_eq!(freshness_serpapi(&Freshness::Month), "qdr:m");
        assert_eq!(freshness_serpapi(&Freshness::Year), "qdr:y");
    }
}
