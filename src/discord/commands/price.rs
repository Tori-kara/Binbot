use poise::serenity_prelude as serenity;

use crate::currency::SUPPORTED_CURRENCIES;
use crate::discord::bot::{Context, Error};
use crate::discord::embeds;

/// Autocomplete suggestions for cryptocurrency symbols based on live in-memory state
async fn autocomplete_symbol(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let mut symbols = ctx.data().market_state.get_symbols().await;

    // If market state is still populating on startup, provide top coins as immediate fallback
    if symbols.is_empty() {
        symbols = vec![
            "BTCUSDT".to_string(),
            "ETHUSDT".to_string(),
            "SOLUSDT".to_string(),
            "BNBUSDT".to_string(),
            "XRPUSDT".to_string(),
            "DOGEUSDT".to_string(),
            "ADAUSDT".to_string(),
            "AVAXUSDT".to_string(),
            "SUIUSDT".to_string(),
            "PEPEUSDT".to_string(),
            "SHIBUSDT".to_string(),
            "LINKUSDT".to_string(),
            "NEARUSDT".to_string(),
        ];
    }

    symbols.sort();

    let query = partial.trim().to_uppercase();

    let choices: Vec<serenity::AutocompleteChoice> = symbols
        .into_iter()
        .filter(|s| query.is_empty() || s.contains(&query))
        .take(25)
        .map(|s| {
            let label = if s.ends_with("USDT") {
                let base = s.trim_end_matches("USDT");
                format!("{base} ({s})")
            } else {
                s.clone()
            };
            serenity::AutocompleteChoice::new(label, s)
        })
        .collect();

    serenity::CreateAutocompleteResponse::new().set_choices(choices)
}

/// Autocomplete suggestions for supported fiat currencies
async fn autocomplete_currency(
    _ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let query = partial.trim().to_uppercase();

    let choices: Vec<serenity::AutocompleteChoice> = SUPPORTED_CURRENCIES
        .iter()
        .filter(|c| {
            query.is_empty()
                || c.code.contains(&query)
                || c.name.to_uppercase().contains(&query)
        })
        .take(25)
        .map(|c| {
            let label = format!("{} {} - {} ({})", c.flag_emoji, c.code, c.name, c.symbol);
            serenity::AutocompleteChoice::new(label, c.code.to_string())
        })
        .collect();

    serenity::CreateAutocompleteResponse::new().set_choices(choices)
}

/// Fetch real-time price and 24h market statistics for a cryptocurrency
#[poise::command(slash_command)]
pub async fn price(
    ctx: Context<'_>,
    #[description = "Cryptocurrency ticker symbol (e.g. BTC, ETH, SOL, or BTCUSDT)"]
    #[autocomplete = "autocomplete_symbol"]
    symbol: String,
    #[description = "Target local fiat currency (e.g. PHP, CAD, JPY, EUR, USD)"]
    #[autocomplete = "autocomplete_currency"]
    currency: Option<String>,
) -> Result<(), Error> {
    let clean = symbol.trim().to_uppercase();

    // In-memory read (< 1ms latency, avoiding Discord 3s timeout)
    let snapshot = match ctx.data().market_state.get_snapshot(&clean).await {
        Some(data) => Some(data),
        None => {
            // If user typed e.g. "BTC", also try "BTCUSDT"
            if !clean.ends_with("USDT") {
                let usdt_pair = format!("{}USDT", clean);
                ctx.data().market_state.get_snapshot(&usdt_pair).await
            } else {
                None
            }
        }
    };

    // Resolve target currency if specified
    let target_curr = if let Some(ref curr_query) = currency {
        ctx.data().currency_service.resolve_currency(curr_query).await
    } else {
        None
    };

    match snapshot {
        Some(data) => {
            let curr_ref = target_curr.as_ref().map(|(info, rate)| (info, *rate));
            let embed = embeds::create_price_embed(&data, curr_ref);
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
        None => {
            let tracked = ctx.data().market_state.get_symbols().await;
            let embed = embeds::create_not_found_embed(&clean, &tracked);
            ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true))
                .await?;
        }
    }

    Ok(())
}
