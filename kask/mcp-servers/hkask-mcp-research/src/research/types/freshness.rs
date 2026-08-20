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
