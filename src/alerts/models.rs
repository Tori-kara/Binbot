use std::str::FromStr;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

/// Default hysteresis reset band: 0.5% (0.005)
pub const DEFAULT_HYSTERESIS_RATE: Decimal = dec!(0.005);

/// Metrics supported in threshold rules
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricTarget {
    Price,
    Volume24h,
    QuoteVolume24h,
}

impl MetricTarget {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Price => "Price",
            Self::Volume24h => "Volume_24h",
            Self::QuoteVolume24h => "Quote_Volume_24h",
        }
    }
}

/// Comparison operators for metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonOp {
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

impl ComparisonOp {
    pub fn symbol_str(&self) -> &'static str {
        match self {
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
        }
    }

    pub fn matches(&self, actual: Decimal, target: Decimal) -> bool {
        match self {
            Self::GreaterThan => actual > target,
            Self::GreaterThanOrEqual => actual >= target,
            Self::LessThan => actual < target,
            Self::LessThanOrEqual => actual <= target,
        }
    }
}

/// Alert condition types supporting single, rolling window, and composite expressions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AlertCondition {
    /// Triggers when price rises to or above the specified threshold (legacy & primary)
    PriceAbove(Decimal),
    /// Triggers when price falls to or below the specified threshold (legacy & primary)
    PriceBelow(Decimal),
    /// Triggers when price moves by the specified percentage from baseline (e.g. +5.0 or -3.0)
    PercentageChange(Decimal),
    /// Triggers on arbitrary metric threshold (Price, Volume_24h, Quote_Volume_24h)
    MetricThreshold {
        metric: MetricTarget,
        op: ComparisonOp,
        value: Decimal,
    },
    /// Triggers when price moves by > X% in a rolling window of Y seconds
    RollingWindowMove {
        percent: Decimal,
        window_seconds: u64,
    },
    /// Multi-condition conjunction: ALL conditions must be met (AND)
    All(Vec<AlertCondition>),
    /// Multi-condition disjunction: ANY condition must be met (OR)
    Any(Vec<AlertCondition>),
}

impl AlertCondition {
    pub fn condition_type_str(&self) -> &'static str {
        match self {
            Self::PriceAbove(_) => "price_above",
            Self::PriceBelow(_) => "price_below",
            Self::PercentageChange(_) => "percentage_change",
            Self::MetricThreshold { .. } => "metric_threshold",
            Self::RollingWindowMove { .. } => "rolling_window",
            Self::All(_) => "compound_all",
            Self::Any(_) => "compound_any",
        }
    }

    pub fn threshold_value(&self) -> Decimal {
        match self {
            Self::PriceAbove(p) | Self::PriceBelow(p) | Self::PercentageChange(p) => *p,
            Self::MetricThreshold { value, .. } => *value,
            Self::RollingWindowMove { percent, .. } => *percent,
            Self::All(items) | Self::Any(items) => {
                items.first().map(|i| i.threshold_value()).unwrap_or(Decimal::ZERO)
            }
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
            Self::MetricThreshold { metric: MetricTarget::Price, value, .. } => *value,
            Self::All(items) | Self::Any(items) => {
                for item in items {
                    let target = item.compute_target_price(baseline_price);
                    if !target.is_zero() {
                        return target;
                    }
                }
                Decimal::ZERO
            }
            _ => Decimal::ZERO,
        }
    }

    /// Whether this condition represents an upward price move
    pub fn is_upward(&self) -> bool {
        match self {
            Self::PriceAbove(_) => true,
            Self::PriceBelow(_) => false,
            Self::PercentageChange(pct) => *pct >= Decimal::ZERO,
            Self::MetricThreshold { op, .. } => matches!(op, ComparisonOp::GreaterThan | ComparisonOp::GreaterThanOrEqual),
            Self::RollingWindowMove { percent, .. } => *percent >= Decimal::ZERO,
            Self::All(items) | Self::Any(items) => {
                items.first().map(|i| i.is_upward()).unwrap_or(true)
            }
        }
    }

    /// Calculates the hysteresis reset band price.
    /// - Upward alerts reset when price drops below `target * (1 - hysteresis)`
    /// - Downward alerts reset when price rises above `target * (1 + hysteresis)`
    pub fn compute_reset_price(&self, baseline_price: Option<Decimal>, hysteresis_rate: Decimal) -> Decimal {
        let target = self.compute_target_price(baseline_price);
        if target.is_zero() {
            return Decimal::ZERO;
        }
        if self.is_upward() {
            target * (Decimal::ONE - hysteresis_rate)
        } else {
            target * (Decimal::ONE + hysteresis_rate)
        }
    }

    /// Human-friendly display string
    pub fn display_string(&self) -> String {
        match self {
            Self::PriceAbove(p) => format!("Price ≥ ${p}"),
            Self::PriceBelow(p) => format!("Price ≤ ${p}"),
            Self::PercentageChange(pct) => format!("{:+}% from baseline", pct),
            Self::MetricThreshold { metric, op, value } => {
                format!("{} {} {}", metric.label(), op.symbol_str(), format_number_abbreviated(*value))
            }
            Self::RollingWindowMove { percent, window_seconds } => {
                let mins = window_seconds / 60;
                if mins >= 60 {
                    format!("Moved ≥ {}% in {}h", percent, mins / 60)
                } else if mins > 0 {
                    format!("Moved ≥ {}% in {}m", percent, mins)
                } else {
                    format!("Moved ≥ {}% in {}s", percent, window_seconds)
                }
            }
            Self::All(items) => items
                .iter()
                .map(|i| i.display_string())
                .collect::<Vec<_>>()
                .join(" AND "),
            Self::Any(items) => items
                .iter()
                .map(|i| i.display_string())
                .collect::<Vec<_>>()
                .join(" OR "),
        }
    }
}

fn format_number_abbreviated(val: Decimal) -> String {
    let abs_val = if val < Decimal::ZERO { -val } else { val };
    let sign = if val < Decimal::ZERO { "-" } else { "" };

    if abs_val >= dec!(1_000_000_000_000) {
        format!("{}{:.2}T", sign, abs_val / dec!(1_000_000_000_000))
    } else if abs_val >= dec!(1_000_000_000) {
        format!("{}{:.2}B", sign, abs_val / dec!(1_000_000_000))
    } else if abs_val >= dec!(1_000_000) {
        format!("{}{:.2}M", sign, abs_val / dec!(1_000_000))
    } else if abs_val >= dec!(1_000) {
        format!("{}{:.2}K", sign, abs_val / dec!(1_000))
    } else {
        format!("{}{}", sign, abs_val)
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

/// Parses unit multiplier suffix (k, m, b, t)
pub fn parse_number_with_suffix(input: &str) -> Result<Decimal, String> {
    let cleaned = input.trim().replace('$', "").replace(',', "");
    if cleaned.is_empty() {
        return Err("Number string is empty".to_string());
    }

    let last_char = cleaned.chars().last().unwrap();
    if last_char.is_alphabetic() {
        let (num_part, multiplier) = match last_char.to_ascii_lowercase() {
            'k' => (&cleaned[..cleaned.len() - 1], dec!(1_000)),
            'm' => (&cleaned[..cleaned.len() - 1], dec!(1_000_000)),
            'b' => (&cleaned[..cleaned.len() - 1], dec!(1_000_000_000)),
            't' => (&cleaned[..cleaned.len() - 1], dec!(1_000_000_000_000)),
            other => return Err(format!("Unknown number multiplier suffix: '{other}'")),
        };
        let val = Decimal::from_str(num_part.trim())
            .map_err(|e| format!("Invalid numeric value '{num_part}': {e}"))?;
        Ok(val * multiplier)
    } else {
        Decimal::from_str(&cleaned).map_err(|e| format!("Invalid numeric value '{cleaned}': {e}"))
    }
}

/// Parses duration strings like "5m", "15min", "1h", "30s" into seconds
pub fn parse_duration_seconds(input: &str) -> Result<u64, String> {
    let lower = input.trim().to_lowercase();
    if lower.is_empty() {
        return Err("Duration string is empty".to_string());
    }

    if lower.ends_with("minutes") || lower.ends_with("minute") || lower.ends_with("mins") || lower.ends_with("min") || lower.ends_with('m') {
        let num_str = lower
            .trim_end_matches("minutes")
            .trim_end_matches("minute")
            .trim_end_matches("mins")
            .trim_end_matches("min")
            .trim_end_matches('m')
            .trim();
        let mins = num_str.parse::<u64>().map_err(|_| format!("Invalid minutes '{num_str}'"))?;
        Ok(mins * 60)
    } else if lower.ends_with("hours") || lower.ends_with("hour") || lower.ends_with("hr") || lower.ends_with('h') {
        let num_str = lower
            .trim_end_matches("hours")
            .trim_end_matches("hour")
            .trim_end_matches("hr")
            .trim_end_matches('h')
            .trim();
        let hours = num_str.parse::<u64>().map_err(|_| format!("Invalid hours '{num_str}'"))?;
        Ok(hours * 3600)
    } else if lower.ends_with("seconds") || lower.ends_with("second") || lower.ends_with("sec") || lower.ends_with('s') {
        let num_str = lower
            .trim_end_matches("seconds")
            .trim_end_matches("second")
            .trim_end_matches("sec")
            .trim_end_matches('s')
            .trim();
        let secs = num_str.parse::<u64>().map_err(|_| format!("Invalid seconds '{num_str}'"))?;
        Ok(secs)
    } else {
        // Default to minutes if just a number
        let mins = lower.parse::<u64>().map_err(|_| format!("Could not parse duration '{input}'"))?;
        Ok(mins * 60)
    }
}

/// Parses a single atomic condition token
fn parse_atomic_condition(input: &str, current_price: Decimal) -> Result<AlertCondition, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("Empty condition token".to_string());
    }

    let lower = raw.to_lowercase();

    // 1. Rolling window move / volatility: e.g. "Price moved > 3% in 5m", "moved > 3% in 5m", "+3% in 5m"
    if lower.contains(" in ") || (lower.contains("moved") && lower.contains('%')) {
        let parts: Vec<&str> = lower.split(" in ").collect();
        if parts.len() == 2 {
            let duration_sec = parse_duration_seconds(parts[1])?;
            let mut left = parts[0].trim();
            if left.starts_with("price") {
                left = left["price".len()..].trim();
            }
            if left.starts_with("moved") {
                left = left["moved".len()..].trim();
            }
            if left.starts_with("volatility") {
                left = left["volatility".len()..].trim();
            }
            let left_cleaned = left.trim_start_matches(">=").trim_start_matches('>').trim_end_matches('%').trim();
            let pct = Decimal::from_str(left_cleaned)
                .map_err(|_| format!("Invalid percentage in rolling window: '{left_cleaned}'"))?;
            return Ok(AlertCondition::RollingWindowMove {
                percent: pct,
                window_seconds: duration_sec,
            });
        }
    }

    // 2. Metric threshold for Volume / Quote Volume
    let (metric_opt, remainder) = if lower.starts_with("volume_24h") || lower.starts_with("volume24h") {
        let stripped = raw.split_at(lower.find("24h").unwrap() + 3).1;
        (Some(MetricTarget::Volume24h), stripped)
    } else if lower.starts_with("quote_volume_24h") || lower.starts_with("quote_volume") || lower.starts_with("quotevol") {
        let idx = if lower.starts_with("quote_volume_24h") {
            "quote_volume_24h".len()
        } else if lower.starts_with("quote_volume") {
            "quote_volume".len()
        } else {
            "quotevol".len()
        };
        (Some(MetricTarget::QuoteVolume24h), &raw[idx..])
    } else if lower.starts_with("volume") || lower.starts_with("vol") {
        let idx = if lower.starts_with("volume") { "volume".len() } else { "vol".len() };
        (Some(MetricTarget::Volume24h), &raw[idx..])
    } else if lower.starts_with("price") {
        (Some(MetricTarget::Price), &raw["price".len()..])
    } else {
        (None, raw)
    };

    if let Some(metric) = metric_opt {
        let rem = remainder.trim();
        let (op, val_str) = if rem.starts_with(">=") {
            (ComparisonOp::GreaterThanOrEqual, &rem[2..])
        } else if rem.starts_with('>') {
            (ComparisonOp::GreaterThan, &rem[1..])
        } else if rem.starts_with("<=") {
            (ComparisonOp::LessThanOrEqual, &rem[2..])
        } else if rem.starts_with('<') {
            (ComparisonOp::LessThan, &rem[1..])
        } else if rem.to_lowercase().starts_with("above ") {
            (ComparisonOp::GreaterThan, &rem[6..])
        } else if rem.to_lowercase().starts_with("below ") {
            (ComparisonOp::LessThan, &rem[6..])
        } else {
            return Err(format!("Expected operator (>, <, >=, <=) after metric in '{raw}'"));
        };

        let val = parse_number_with_suffix(val_str.trim())?;
        if metric == MetricTarget::Price {
            if matches!(op, ComparisonOp::GreaterThan | ComparisonOp::GreaterThanOrEqual) {
                return Ok(AlertCondition::PriceAbove(val));
            } else {
                return Ok(AlertCondition::PriceBelow(val));
            }
        } else {
            return Ok(AlertCondition::MetricThreshold {
                metric,
                op,
                value: val,
            });
        }
    }

    // 3. Percentage formats: +5%, -3%, 5%
    if raw.ends_with('%') {
        let num_part = raw.trim_end_matches('%').trim();
        let val = Decimal::from_str(num_part)
            .map_err(|_| format!("Invalid percentage value: '{num_part}'"))?;
        return Ok(AlertCondition::PercentageChange(val));
    }

    if (raw.starts_with('+') || (raw.starts_with('-') && !raw.contains('.'))) && !raw.contains('>') && !raw.contains('<') {
        if let Ok(val) = Decimal::from_str(raw) {
            if raw.starts_with('+') {
                return Ok(AlertCondition::PercentageChange(val));
            }
        }
    }

    // 4. Direct Price operators without "Price" keyword: > 100k, >= 4000, < 2500, above 4000
    if raw.starts_with(">=") {
        let val = parse_number_with_suffix(&raw[2..])?;
        return Ok(AlertCondition::PriceAbove(val));
    } else if raw.starts_with('>') {
        let val = parse_number_with_suffix(&raw[1..])?;
        return Ok(AlertCondition::PriceAbove(val));
    } else if lower.starts_with("above ") {
        let val = parse_number_with_suffix(&raw[6..])?;
        return Ok(AlertCondition::PriceAbove(val));
    } else if raw.starts_with("<=") {
        let val = parse_number_with_suffix(&raw[2..])?;
        return Ok(AlertCondition::PriceBelow(val));
    } else if raw.starts_with('<') {
        let val = parse_number_with_suffix(&raw[1..])?;
        return Ok(AlertCondition::PriceBelow(val));
    } else if lower.starts_with("below ") {
        let val = parse_number_with_suffix(&raw[6..])?;
        return Ok(AlertCondition::PriceBelow(val));
    }

    // 5. Plain number: infer based on current market price
    if let Ok(val) = parse_number_with_suffix(raw) {
        if val > current_price {
            return Ok(AlertCondition::PriceAbove(val));
        } else {
            return Ok(AlertCondition::PriceBelow(val));
        }
    }

    Err(format!(
        "Could not parse condition '{}'. Examples: 'Price > 100k AND Volume_24h > 50B', 'Price moved > 3% in 5m', '+5%', '> 4000'",
        raw
    ))
}

/// Parses user command input string into an `AlertCondition` (supporting compound AND/OR expressions)
pub fn parse_condition(input: &str, current_price: Decimal) -> Result<AlertCondition, String> {
    AlertCondition::parse(input, current_price)
}

impl AlertCondition {
    /// Convenience helper for tests
    pub fn price_above(val: Decimal) -> Self {
        Self::PriceAbove(val)
    }

    pub fn price_below(val: Decimal) -> Self {
        Self::PriceBelow(val)
    }

    pub fn parse(input: &str, current_price: Decimal) -> Result<Self, String> {
        let raw = input.trim();
        if raw.is_empty() {
            return Err("Alert condition cannot be empty".to_string());
        }

        // Split on case-insensitive " AND "
        let and_split: Vec<&str> = split_case_insensitive(raw, " and ");
        if and_split.len() > 1 {
            let mut list = Vec::new();
            for part in and_split {
                list.push(parse_atomic_condition(part, current_price)?);
            }
            return Ok(AlertCondition::All(list));
        }

        // Split on case-insensitive " OR "
        let or_split: Vec<&str> = split_case_insensitive(raw, " or ");
        if or_split.len() > 1 {
            let mut list = Vec::new();
            for part in or_split {
                list.push(parse_atomic_condition(part, current_price)?);
            }
            return Ok(AlertCondition::Any(list));
        }

        parse_atomic_condition(raw, current_price)
    }
}

fn split_case_insensitive<'a>(input: &'a str, delim: &str) -> Vec<&'a str> {
    let lower_input = input.to_lowercase();
    let lower_delim = delim.to_lowercase();
    let mut results = Vec::new();
    let mut last_idx = 0;

    for (start, _) in lower_input.match_indices(&lower_delim) {
        results.push(input[last_idx..start].trim());
        last_idx = start + delim.len();
    }
    results.push(input[last_idx..].trim());
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_multi_condition_and() {
        let cur = dec!(95000);
        let cond = AlertCondition::parse("Price > 100k AND Volume_24h > 50B", cur).unwrap();

        match cond {
            AlertCondition::All(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], AlertCondition::PriceAbove(dec!(100000)));
                assert_eq!(
                    items[1],
                    AlertCondition::MetricThreshold {
                        metric: MetricTarget::Volume24h,
                        op: ComparisonOp::GreaterThan,
                        value: dec!(50000000000),
                    }
                );
            }
            other => panic!("Expected AlertCondition::All, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_multi_condition_quote_volume() {
        let cur = dec!(95000);
        let cond = AlertCondition::parse("price > 100k and quote_volume > 10b", cur).unwrap();
        match cond {
            AlertCondition::All(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], AlertCondition::PriceAbove(dec!(100000)));
                assert_eq!(
                    items[1],
                    AlertCondition::MetricThreshold {
                        metric: MetricTarget::QuoteVolume24h,
                        op: ComparisonOp::GreaterThan,
                        value: dec!(10000000000),
                    }
                );
            }
            other => panic!("Expected AlertCondition::All, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_rolling_window_volatility() {
        let cur = dec!(3000);
        let cond = AlertCondition::parse("Price moved > 3% in 5 minutes", cur).unwrap();
        assert_eq!(
            cond,
            AlertCondition::RollingWindowMove {
                percent: dec!(3),
                window_seconds: 300,
            }
        );

        let cond2 = AlertCondition::parse("+5% in 15m", cur).unwrap();
        assert_eq!(
            cond2,
            AlertCondition::RollingWindowMove {
                percent: dec!(5),
                window_seconds: 900,
            }
        );
    }

    #[test]
    fn test_parse_number_with_suffix() {
        assert_eq!(parse_number_with_suffix("100k").unwrap(), dec!(100000));
        assert_eq!(parse_number_with_suffix("2.5M").unwrap(), dec!(2500000));
        assert_eq!(parse_number_with_suffix("$50B").unwrap(), dec!(50000000000));
        assert_eq!(parse_number_with_suffix("1T").unwrap(), dec!(1000000000000));
        assert_eq!(parse_number_with_suffix("4200.50").unwrap(), dec!(4200.50));
    }

    #[test]
    fn test_parse_legacy_conditions() {
        let cur = dec!(3000);
        assert_eq!(
            AlertCondition::parse("> 4000", cur).unwrap(),
            AlertCondition::PriceAbove(dec!(4000))
        );
        assert_eq!(
            AlertCondition::parse("< 2500", cur).unwrap(),
            AlertCondition::PriceBelow(dec!(2500))
        );
        assert_eq!(
            AlertCondition::parse("+5%", cur).unwrap(),
            AlertCondition::PercentageChange(dec!(5))
        );
        assert_eq!(
            AlertCondition::parse("-3%", cur).unwrap(),
            AlertCondition::PercentageChange(dec!(-3))
        );
        assert_eq!(
            AlertCondition::parse("3500", cur).unwrap(),
            AlertCondition::PriceAbove(dec!(3500))
        );
        assert_eq!(
            AlertCondition::parse("2500", cur).unwrap(),
            AlertCondition::PriceBelow(dec!(2500))
        );
    }
}
