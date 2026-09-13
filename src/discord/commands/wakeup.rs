use crate::discord::bot::{Context, Error};
use crate::discord::embeds;

/// Check status of Render service & Binbot backend, waking up or confirming active state
#[poise::command(slash_command)]
pub async fn wakeup(ctx: Context<'_>) -> Result<(), Error> {
    let tracked_count = ctx.data().market_state.count().await;
    let fiat_rates = ctx.data().currency_service.get_supported_rates().await;

    let embed = embeds::create_wakeup_embed(tracked_count, fiat_rates.len());
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
