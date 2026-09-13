use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serenity::all::{CreateEmbed, CreateEmbedFooter, Timestamp};

use crate::currency::{CurrencyInfo, CurrencyService};
use crate::market::MarketData;

fn format_usd_price(val: Decimal) -> String {
    if val >= Decimal::from(1) {
        format!("${:.2}", val)
    } else {
        format!("${:.6}", val)
    }
}

pub fn create_price_embed(
    data: &MarketData,
    currency_opt: Option<(&CurrencyInfo, Decimal)>,
) -> CreateEmbed {
    let is_positive = data.price_change_percent_24hr >= Decimal::ZERO;
    let color = if is_positive { 0x2ECC71 } else { 0xE74C3C };

    let trend_emoji = if is_positive { "📈" } else { "📉" };
    let change_sign = if is_positive { "+" } else { "" };

    let ts = Timestamp::from_unix_timestamp(data.update_at.timestamp())
        .unwrap_or_else(|_| Timestamp::now());

    match currency_opt {
        Some((curr, rate)) if curr.code != "USD" => {
            // Local currency conversions
            let local_price = data.price * rate;
            let local_change = data.price_change_24hr * rate;
            let local_high = data.high_price_24hr * rate;
            let local_low = data.low_price_24hr * rate;
            let local_quote_vol = data.quote_volume_24hr * rate;

            let price_str = format!(
                "**{}** {}\n*({} USD)*",
                CurrencyService::format_amount(local_price, curr),
                curr.code,
                format_usd_price(data.price)
            );

            let change_str = format!(
                "{}{:.2}%\n({})",
                change_sign,
                data.price_change_percent_24hr,
                CurrencyService::format_amount(local_change, curr)
            );

            let high_low_str = format!(
                "High: {}\nLow: {}",
                CurrencyService::format_amount(local_high, curr),
                CurrencyService::format_amount(local_low, curr)
            );

            let volume_str = format!(
                "{:.2} base\n{} quote",
                data.volume_24hr,
                CurrencyService::format_amount(local_quote_vol, curr)
            );

            let footer_text = format!(
                "Binbot FX Engine • 1 USD ≈ {} {} • Binance In-Memory",
                CurrencyService::format_amount(rate, curr),
                curr.code
            );

            CreateEmbed::new()
                .title(format!(
                    "{} {} Market Summary ({} {})",
                    trend_emoji, data.symbol, curr.flag_emoji, curr.code
                ))
                .color(color)
                .field("Current Price", price_str, true)
                .field("24hr Change", change_str, true)
                .field("24hr Range", high_low_str, true)
                .field("24hr Volume", volume_str, true)
                .footer(CreateEmbedFooter::new(footer_text))
                .timestamp(ts)
        }
        _ => {
            // Standard USD embed
            let change_str = format!(
                "{}{:.2}% ({}{})",
                change_sign,
                data.price_change_percent_24hr,
                change_sign,
                format_usd_price(data.price_change_24hr)
            );

            let high_low_str = format!(
                "High: {}\nLow: {}",
                format_usd_price(data.high_price_24hr),
                format_usd_price(data.low_price_24hr)
            );

            let volume_str = format!(
                "{:.2} base\n${:.2} quote",
                data.volume_24hr,
                data.quote_volume_24hr
            );

            CreateEmbed::new()
                .title(format!("{} {} Market Summary", trend_emoji, data.symbol))
                .color(color)
                .field("Current Price", format_usd_price(data.price), true)
                .field("24hr Change", change_str, true)
                .field("24hr Range", high_low_str, true)
                .field("24hr Volume", volume_str, true)
                .footer(CreateEmbedFooter::new("Binbot • Binance In-Memory Engine"))
                .timestamp(ts)
        }
    }
}

pub fn create_currencies_embed(
    rates: &[(CurrencyInfo, Decimal)],
    last_updated: DateTime<Utc>,
    last_checked: DateTime<Utc>,
) -> CreateEmbed {
    let mut lines = Vec::new();
    for (curr, rate) in rates {
        let rate_str = if curr.code == "USD" {
            "Base Currency (1.00)".to_string()
        } else {
            format!("1 USD = {}", CurrencyService::format_amount(*rate, curr))
        };
        lines.push(format!(
            "{} **{}** — {} (`{}`)",
            curr.flag_emoji, curr.code, curr.name, rate_str
        ));
    }

    let description = format!(
        "Track live crypto prices converted into your local currency!\n\n\
        **Usage:**\n\
        Use `/price <symbol> currency:<CODE>`\n\
        *Example: `/price BTC currency:PHP` or `/price ETH currency:CAD`*\n\n\
        **Supported Currencies & Live Exchange Rates:**\n{}",
        lines.join("\n")
    );

    let ts = Timestamp::from_unix_timestamp(last_updated.timestamp())
        .unwrap_or_else(|_| Timestamp::now());

    let footer_text = format!(
        "Binbot FX Engine • Checked: {} • Updated: {}",
        last_checked.format("%b %d %H:%M UTC"),
        last_updated.format("%b %d %H:%M UTC")
    );

    CreateEmbed::new()
        .title("💱 Supported Local Currencies")
        .description(description)
        .color(0x3498DB)
        .footer(CreateEmbedFooter::new(footer_text))
        .timestamp(ts)
}

pub fn create_not_found_embed(query: &str, tracked_symbols: &[String]) -> CreateEmbed {
    let list = if tracked_symbols.is_empty() {
        "No symbols are currently being tracked.".to_string()
    } else {
        tracked_symbols
            .iter()
            .map(|s| format!("`{}`", s))
            .collect::<Vec<_>>()
            .join(", ")
    };

    CreateEmbed::new()
        .title("Symbol Not Found")
        .description(format!(
            "Could not find live market data for **`{}`**.\n\n**Currently Tracked Symbols:**\n{}",
            query.to_uppercase(),
            list
        ))
        .color(0xF39C12)
        .footer(CreateEmbedFooter::new("BinBot • Symbol Lookup"))
        .timestamp(Timestamp::from_unix_timestamp(Utc::now().timestamp()).unwrap_or_else(|_| Timestamp::now()))
}

pub fn create_alert_triggered_embed(notif: &crate::alerts::AlertNotification) -> CreateEmbed {
    let is_upward = notif.condition.is_upward();
    let color = if is_upward { 0x2ECC71 } else { 0xE74C3C };
    let trend_emoji = if is_upward { "🚀" } else { "🔻" };

    let condition_str = match notif.condition {
        crate::alerts::AlertCondition::PriceAbove(p) => format!("Price Above {}", format_usd_price(p)),
        crate::alerts::AlertCondition::PriceBelow(p) => format!("Price Below {}", format_usd_price(p)),
        crate::alerts::AlertCondition::PercentageChange(pct) => {
            let base_str = notif
                .baseline_price
                .map(format_usd_price)
                .unwrap_or_else(|| "N/A".to_string());
            format!("Move of {:+}% (from {})", pct, base_str)
        }
    };

    let reset_price = notif
        .condition
        .compute_reset_price(notif.baseline_price, crate::alerts::DEFAULT_HYSTERESIS_RATE);

    let reset_desc = if is_upward {
        format!("Drops below {} (0.5% hysteresis)", format_usd_price(reset_price))
    } else {
        format!("Rises above {} (0.5% hysteresis)", format_usd_price(reset_price))
    };

    let prev_str = notif
        .previous_price
        .map(format_usd_price)
        .unwrap_or_else(|| "N/A".to_string());

    let ts = Timestamp::from_unix_timestamp(notif.triggered_at.timestamp())
        .unwrap_or_else(|_| Timestamp::now());

    CreateEmbed::new()
        .title(format!("{} Alert Triggered: {}", trend_emoji, notif.symbol))
        .color(color)
        .field("Triggered Price", format_usd_price(notif.trigger_price), true)
        .field("Previous Price", prev_str, true)
        .field("Target Condition", condition_str, false)
        .field("Hysteresis Reset Band", reset_desc, false)
        .footer(CreateEmbedFooter::new(format!(
            "Binbot Alert Engine • Alert #{} • Cooldown Active",
            notif.alert_id
        )))
        .timestamp(ts)
}

pub fn create_watch_success_embed(alert: &crate::alerts::Alert, current_price: Decimal) -> CreateEmbed {
    let target = alert.target_price();
    let reset = alert.reset_price(crate::alerts::DEFAULT_HYSTERESIS_RATE);

    let condition_str = match alert.condition {
        crate::alerts::AlertCondition::PriceAbove(p) => format!("Price Above {}", format_usd_price(p)),
        crate::alerts::AlertCondition::PriceBelow(p) => format!("Price Below {}", format_usd_price(p)),
        crate::alerts::AlertCondition::PercentageChange(pct) => {
            format!("{:+}% from baseline", pct)
        }
    };

    let reset_desc = if alert.is_upward() {
        format!("Re-arms when drops below {}", format_usd_price(reset))
    } else {
        format!("Re-arms when rises above {}", format_usd_price(reset))
    };

    CreateEmbed::new()
        .title(format!("🔔 Watch Alert Created: {}", alert.symbol))
        .description(format!(
            "Alert **#{}** registered! You will be notified in this channel when **{}** reaches the threshold.",
            alert.id, alert.symbol
        ))
        .color(0x3498DB)
        .field("Current Price", format_usd_price(current_price), true)
        .field("Target Price", format_usd_price(target), true)
        .field("Condition", condition_str, true)
        .field("Hysteresis Reset", reset_desc, false)
        .field(
            "Cooldown",
            format!("{} minutes", alert.cooldown_seconds / 60),
            true,
        )
        .field("Status", "🟢 Armed", true)
        .footer(CreateEmbedFooter::new("Binbot • Automated Market Monitor"))
        .timestamp(Timestamp::now())
}

pub fn create_alert_list_embed(alerts: &[crate::alerts::Alert], username: &str) -> CreateEmbed {
    if alerts.is_empty() {
        return CreateEmbed::new()
            .title(format!("🔔 Active Alerts for {}", username))
            .description("You do not have any active alerts.\nUse `/watch <symbol> <condition>` to set one!")
            .color(0x3498DB)
            .footer(CreateEmbedFooter::new("Binbot • Alerts Manager"));
    }

    let mut lines = Vec::new();
    for a in alerts {
        let status = if a.is_triggered {
            "🔴 Waiting Reset"
        } else {
            "🟢 Armed"
        };

        let cond = match a.condition {
            crate::alerts::AlertCondition::PriceAbove(p) => format!("Above {}", format_usd_price(p)),
            crate::alerts::AlertCondition::PriceBelow(p) => format!("Below {}", format_usd_price(p)),
            crate::alerts::AlertCondition::PercentageChange(pct) => format!("{:+}%", pct),
        };

        lines.push(format!(
            "**#{}** • **`{}`** — {} (Target: {}) • {}\n*Cooldown: {}m | Channel: <#{}>*",
            a.id,
            a.symbol,
            cond,
            format_usd_price(a.target_price()),
            status,
            a.cooldown_seconds / 60,
            a.channel_discord_id
        ));
    }

    CreateEmbed::new()
        .title(format!("🔔 Active Alerts for {} ({})", username, alerts.len()))
        .description(format!(
            "{}\n\n*To remove an alert, use `/alerts delete <id>`*",
            lines.join("\n\n")
        ))
        .color(0x3498DB)
        .footer(CreateEmbedFooter::new("Binbot • Alerts Manager"))
        .timestamp(Timestamp::now())
}