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
    #[description = "Condition (e.g. +5%, Price > 100k AND Volume_24h > 50B, Moved > 3% in 5m)"]
    condition: String,
    #[description = "Cooldown period in minutes before re-triggering (default: 30)"]
    cooldown_minutes: Option<u32>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let clean_symbol = symbol.trim().to_uppercase();

    // Fast in-memory lookup (< 1ms, auto-resolves symbol and symbolUSDT)
    let snapshot = match ctx.data().market_state.get_snapshot(&clean_symbol).await {
        Some(s) => s,
        None => {
            let tracked = ctx.data().market_state.get_symbols().await;
            let embed = embeds::create_not_found_embed(&clean_symbol, &tracked);
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }
    };

    let current_price = snapshot.price;

    // Parse the condition input (supporting composite and rolling conditions)
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

    // If condition contains PercentageChange, record current price as baseline
    fn has_percentage_change(c: &AlertCondition) -> bool {
        match c {
            AlertCondition::PercentageChange(_) => true,
            AlertCondition::All(items) | AlertCondition::Any(items) => items.iter().any(has_percentage_change),
            _ => false,
        }
    }

    let baseline_price = if has_percentage_change(&parsed_cond) {
        Some(current_price)
    } else {
        None
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
