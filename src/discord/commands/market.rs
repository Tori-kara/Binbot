use chrono::Utc;
use crate::discord::bot::{Context, Error};
use crate::discord::embeds;
use crate::market::compile_market_digest;

/// Cryptocurrency market intelligence, top gainers/losers, and scheduled daily digests
#[poise::command(
    slash_command,
    subcommands("overview", "digest"),
    subcommand_required
)]
pub async fn market(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// View the live global cryptocurrency market summary and top movers
#[poise::command(slash_command)]
pub async fn overview(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let snapshots = ctx.data().market_state.get_all_snapshots().await;
    if snapshots.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .content("Market data engine is currently synchronizing with Binance. Please try again in a few moments.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let digest = compile_market_digest(&snapshots, Utc::now());
    let embed = embeds::create_market_overview_embed(&digest, false);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Manage automated daily market digests (delivered every morning at 08:00 UTC)
#[poise::command(
    slash_command,
    subcommands("subscribe", "unsubscribe", "status"),
    subcommand_required
)]
pub async fn digest(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Subscribe this channel to receive daily market intelligence digests at 08:00 UTC
#[poise::command(slash_command)]
pub async fn subscribe(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let channel_discord_id = ctx.channel_id().to_string();
    let channel_name = ctx.channel_id().name(&ctx).await.unwrap_or_else(|_| "digest-channel".to_string());
    let guild_discord_id = ctx.guild_id().map(|g| g.to_string());
    let guild_name = ctx.guild().map(|g| g.name.clone());

    let repo = ctx.data().alert_store.repo();

    // Ensure guild and channel records exist
    let db_guild_id = if let Some(ref gid) = guild_discord_id {
        let name = guild_name.as_deref().unwrap_or("Discord Server");
        Some(repo.guilds().upsert(gid, name).await?)
    } else {
        None
    };

    let db_channel_id = repo
        .channels()
        .upsert(db_guild_id, &channel_discord_id, &channel_name)
        .await?;

    // Subscribe to 08:00 UTC digest
    repo.digests().subscribe(db_guild_id, db_channel_id, "08:00").await?;

    let embed = embeds::create_digest_subscription_embed(&channel_discord_id, true);
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Unsubscribe this channel from scheduled daily market digests
#[poise::command(slash_command)]
pub async fn unsubscribe(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let channel_discord_id = ctx.channel_id().to_string();
    let repo = ctx.data().alert_store.repo();

    let unsubscribed = repo.digests().unsubscribe(&channel_discord_id).await?;

    if unsubscribed {
        let embed = embeds::create_digest_subscription_embed(&channel_discord_id, false);
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    } else {
        ctx.send(
            poise::CreateReply::default()
                .content("ℹ This channel is not currently subscribed to daily market digests.")
                .ephemeral(true),
        )
        .await?;
    }

    Ok(())
}

/// Check whether this channel is subscribed to the 08:00 UTC daily market digest
#[poise::command(slash_command)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let channel_discord_id = ctx.channel_id().to_string();
    let is_sub = ctx
        .data()
        .alert_store
        .repo()
        .digests()
        .is_subscribed(&channel_discord_id)
        .await
        .unwrap_or(false);

    let status_msg = if is_sub {
        format!("🟢 Channel <#{channel_discord_id}> is **subscribed** to daily digests at **08:00 UTC**.")
    } else {
        format!("⚪ Channel <#{channel_discord_id}> is **not subscribed**. Use `/market digest subscribe` to activate.")
    };

    ctx.send(poise::CreateReply::default().content(status_msg).ephemeral(true)).await?;
    Ok(())
}
