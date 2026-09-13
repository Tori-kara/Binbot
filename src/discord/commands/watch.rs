use crate::alerts::models::{parse_condition, AlertCondition};
use crate::discord::bot::{Context, Error};
use crate::discord::commands::price::autocomplete_symbol;
use crate::discord::embeds;

/// Set an automated price alert with edge triggering and cooldown management
#[poise::command(slash_command)]
pub async fn watch(
    ctx: Context<'_>,
    #[description = "Cryptocurrency ticker symbol (e.g. BTC, ETH, SOL)"]
    #[autocomplete = "autocomplete_symbol"]
    symbol: String,
    #[description = "Alert condition (e.g. +5%, -3%, > 100000, < 4000)"]
    condition: String,
    #[description = "Cooldown period in minutes before re-triggering (default: 30)"]
    cooldown_minutes: Option<u32>,
) -> Result<(), Error> {
    let clean_symbol = symbol.trim().to_uppercase();

    // Check if symbol exists in in-memory market state
    let snapshot = match ctx.data().market_state.get_snapshot(&clean_symbol).await {
        Some(s) => Some(s),
        None => {
            if !clean_symbol.ends_with("USDT") {
                let usdt_pair = format!("{}USDT", clean_symbol);
                ctx.data().market_state.get_snapshot(&usdt_pair).await
            } else {
                None
            }
        }
    };

    let snapshot = match snapshot {
        Some(s) => s,
        None => {
            let tracked = ctx.data().market_state.get_symbols().await;
            let embed = embeds::create_not_found_embed(&clean_symbol, &tracked);
            ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true)).await?;
            return Ok(());
        }
    };

    let current_price = snapshot.price;

    // Parse the condition input
    let parsed_cond = match parse_condition(&condition, current_price) {
        Ok(c) => c,
        Err(err_msg) => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("❌ **Invalid Condition:** {err_msg}"))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    // For PercentageChange, save baseline price as current market price
    let baseline_price = match parsed_cond {
        AlertCondition::PercentageChange(_) => Some(current_price),
        _ => None,
    };

    let cooldown_secs = cooldown_minutes.unwrap_or(30).max(1) * 60;

    let user_discord_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let channel_discord_id = ctx.channel_id().to_string();
    let channel_name = ctx.channel_id().name(&ctx).await.unwrap_or_else(|_| "alert-channel".to_string());
    let guild_discord_id = ctx.guild_id().map(|g| g.to_string());
    let guild_name = ctx.guild().map(|g| g.name.clone());

    let alert = match ctx
        .data()
        .alert_store
        .create_alert(
            guild_discord_id.as_deref(),
            guild_name.as_deref(),
            &channel_discord_id,
            &channel_name,
            &user_discord_id,
            &username,
            &snapshot.symbol,
            parsed_cond,
            baseline_price,
            cooldown_secs,
        )
        .await
    {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("Failed to create alert in database: {e}");
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("❌ Failed to register alert due to database error: {e}"))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    let embed = embeds::create_watch_success_embed(&alert, current_price);
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
