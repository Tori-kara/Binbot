use std::str::FromStr;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

/// Default hysteresis reset band: 0.5% (0.005)
pub const DEFAULT_HYSTERESIS_RATE: Decimal = dec!(0.005);

/// Alert condition types
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AlertCondition {
    /// Triggers when price rises to or above the specified threshold
    PriceAbove(Decimal),
    /// Triggers when price falls to or below the specified threshold
    PriceBelow(Decimal),
    /// Triggers when price moves by the specified percentage from baseline (e.g. +5.0 or -3.0)
    PercentageChange(Decimal),
}

impl AlertCondition {
    pub fn condition_type_str(&self) -> &'static str {
        match self {
            Self::PriceAbove(_) => "price_above",
            Self::PriceBelow(_) => "price_below",
            Self::PercentageChange(_) => "percentage_change",
        }
    }

    pub fn threshold_value(&self) -> Decimal {
        match self {
            Self::PriceAbove(p) | Self::PriceBelow(p) | Self::PercentageChange(p) => *p,
        }
    }

    pub fn from_parts(condition_type: &str, threshold: Decimal) -> Result<Self, String> {
        match condition_type.to_lowercase().as_str() {
            "price_above" | "above" | ">" => Ok(Self::PriceAbove(threshold)),
            "price_below" | "below" | "<" => Ok(Self::PriceBelow(threshold)),
            "percentage_change" | "percent" | "%" => Ok(Self::PercentageChange(threshold)),
            other => Err(format!("Unknown alert condition type: {other}")),
        }
    }

    /// Computes the absolute target price given a baseline price
    pub fn compute_target_price(&self, baseline_price: Option<Decimal>) -> Decimal {
        match self {
            Self::PriceAbove(p) | Self::PriceBelow(p) => *p,
            Self::PercentageChange(pct) => {
                let base = baseline_price.unwrap_or(Decimal::ZERO);
                base * (Decimal::ONE + (*pct / dec!(100)))
            }
        }
    }

    /// Whether this condition represents an upward price move
    pub fn is_upward(&self) -> bool {
        match self {
            Self::PriceAbove(_) => true,
            Self::PriceBelow(_) => false,
            Self::PercentageChange(pct) => *pct >= Decimal::ZERO,
        }
    }

    /// Calculates the hysteresis reset band price.
    /// - Upward alerts reset when price drops below `target * (1 - hysteresis)`
    /// - Downward alerts reset when price rises above `target * (1 + hysteresis)`
    pub fn compute_reset_price(&self, baseline_price: Option<Decimal>, hysteresis_rate: Decimal) -> Decimal {
        let target = self.compute_target_price(baseline_price);
        if self.is_upward() {
            target * (Decimal::ONE - hysteresis_rate)
        } else {
            target * (Decimal::ONE + hysteresis_rate)
        }
    }
}

/// Domain model representing an alert in storage and memory
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub id: i64,
    pub user_id: i64,
    pub user_discord_id: String,
    pub channel_id: i64,
    pub channel_discord_id: String,
    pub symbol: String,
    pub condition: AlertCondition,
    pub threshold: Decimal,
    pub baseline_price: Option<Decimal>,
    pub cooldown_seconds: u32,
    pub last_triggered_at: Option<DateTime<Utc>>,
    pub is_triggered: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Alert {
    pub fn target_price(&self) -> Decimal {
        self.condition.compute_target_price(self.baseline_price)
    }

    pub fn reset_price(&self, hysteresis_rate: Decimal) -> Decimal {
        self.condition.compute_reset_price(self.baseline_price, hysteresis_rate)
    }

    pub fn is_upward(&self) -> bool {
        self.condition.is_upward()
    }
}

/// Parses user command input string into an `AlertCondition`.
///
/// Supported formats:
/// - `+5%`, `+5`, `-3%`, `-3`, `5%` -> `PercentageChange`
/// - `> 4000`, `>4000`, `above 4000` -> `PriceAbove(4000)`
/// - `< 2500`, `<2500`, `below 2500` -> `PriceBelow(2500)`
/// - `4000` -> `PriceAbove(4000)` if 4000 > current_price, else `PriceBelow(4000)`
pub fn parse_condition(input: &str, current_price: Decimal) -> Result<AlertCondition, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("Alert condition cannot be empty".to_string());
    }

    // 1. Percentage formats: +5%, -3%, 5%, +5, -3
    if raw.ends_with('%') {
        let num_part = raw.trim_end_matches('%').trim();
        let val = Decimal::from_str(num_part)
            .map_err(|_| format!("Invalid percentage value: '{num_part}'"))?;
        return Ok(AlertCondition::PercentageChange(val));
    }

    if (raw.starts_with('+') || (raw.starts_with('-') && !raw.contains('.'))) && !raw.contains('>') && !raw.contains('<') {
        if let Ok(val) = Decimal::from_str(raw) {
            // Numbers with explicit + are percentage change (e.g. +5 or +5.0)
            if raw.starts_with('+') {
                return Ok(AlertCondition::PercentageChange(val));
            }
        }
    }

    // 2. Explicit Above: > 4000, >= 4000, above 4000
    if raw.starts_with('>') || raw.to_lowercase().starts_with("above ") {
        let stripped = if raw.starts_with(">=") {
            raw.trim_start_matches(">=")
        } else if raw.starts_with('>') {
            raw.trim_start_matches('>')
        } else {
            &raw[6..]
        };
        let val = Decimal::from_str(stripped.trim())
            .map_err(|_| format!("Invalid price for 'above' condition: '{stripped}'"))?;
        return Ok(AlertCondition::PriceAbove(val));
    }

    // 3. Explicit Below: < 2500, <= 2500, below 2500
    if raw.starts_with('<') || raw.to_lowercase().starts_with("below ") {
        let stripped = if raw.starts_with("<=") {
            raw.trim_start_matches("<=")
        } else if raw.starts_with('<') {
            raw.trim_start_matches('<')
        } else {
            &raw[6..]
        };
        let val = Decimal::from_str(stripped.trim())
            .map_err(|_| format!("Invalid price for 'below' condition: '{stripped}'"))?;
        return Ok(AlertCondition::PriceBelow(val));
    }

    // 4. Plain number: infer based on current market price
    if let Ok(val) = Decimal::from_str(raw) {
        if val > current_price {
            return Ok(AlertCondition::PriceAbove(val));
        } else {
            return Ok(AlertCondition::PriceBelow(val));
        }
    }

    Err(format!(
        "Could not parse condition '{}'. Examples: '+5%', '-3%', '> 4000', '< 2500'",
        raw
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_percentage_conditions() {
        let cur = dec!(100000);
        assert_eq!(
            parse_condition("+5%", cur).unwrap(),
            AlertCondition::PercentageChange(dec!(5))
        );
        assert_eq!(
            parse_condition("-3.5%", cur).unwrap(),
            AlertCondition::PercentageChange(dec!(-3.5))
        );
        assert_eq!(
            parse_condition("10%", cur).unwrap(),
            AlertCondition::PercentageChange(dec!(10))
        );
        assert_eq!(
            parse_condition("+2.5", cur).unwrap(),
            AlertCondition::PercentageChange(dec!(2.5))
        );
    }

    #[test]
    fn test_parse_price_above_below() {
        let cur = dec!(3000);
        assert_eq!(
            parse_condition("> 4000", cur).unwrap(),
            AlertCondition::PriceAbove(dec!(4000))
        );
        assert_eq!(
            parse_condition(">4000", cur).unwrap(),
            AlertCondition::PriceAbove(dec!(4000))
        );
        assert_eq!(
            parse_condition("above 4000", cur).unwrap(),
            AlertCondition::PriceAbove(dec!(4000))
        );
        assert_eq!(
            parse_condition("< 2500", cur).unwrap(),
            AlertCondition::PriceBelow(dec!(2500))
        );
        assert_eq!(
            parse_condition("below 2500", cur).unwrap(),
            AlertCondition::PriceBelow(dec!(2500))
        );
    }

    #[test]
    fn test_parse_plain_number_inference() {
        let cur = dec!(3000);
        assert_eq!(
            parse_condition("3500", cur).unwrap(),
            AlertCondition::PriceAbove(dec!(3500))
        );
        assert_eq!(
            parse_condition("2500", cur).unwrap(),
            AlertCondition::PriceBelow(dec!(2500))
        );
    }

    #[test]
    fn test_compute_target_and_reset_price() {
        let cond_above = AlertCondition::PriceAbove(dec!(100000));
        assert_eq!(cond_above.compute_target_price(None), dec!(100000));
        // Reset band with 0.5% (0.005) -> 100000 * 0.995 = 99500
        assert_eq!(
            cond_above.compute_reset_price(None, DEFAULT_HYSTERESIS_RATE),
            dec!(99500)
        );

        let cond_below = AlertCondition::PriceBelow(dec!(4000));
        assert_eq!(cond_below.compute_target_price(None), dec!(4000));
        // Reset band with 0.5% (0.005) -> 4000 * 1.005 = 4020
        assert_eq!(
            cond_below.compute_reset_price(None, DEFAULT_HYSTERESIS_RATE),
            dec!(4020)
        );

        let cond_pct = AlertCondition::PercentageChange(dec!(5));
        // 100000 * (1 + 0.05) = 105000
        assert_eq!(cond_pct.compute_target_price(Some(dec!(100000))), dec!(105000));
        // Reset band: 105000 * 0.995 = 104475
        assert_eq!(
            cond_pct.compute_reset_price(Some(dec!(100000)), DEFAULT_HYSTERESIS_RATE),
            dec!(104475)
        );
    }
}
