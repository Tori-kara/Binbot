use crate::discord::bot::{Context, Error};
use crate::discord::embeds;

/// Display all supported local currencies and their active exchange rates
#[poise::command(slash_command)]
pub async fn currencies(ctx: Context<'_>) -> Result<(), Error> {
    let supported = ctx.data().currency_service.get_supported_rates().await;
    let last_updated = ctx.data().currency_service.last_updated().await;
    let last_checked = ctx.data().currency_service.last_checked().await;

    let embed = embeds::create_currencies_embed(&supported, last_updated, last_checked);
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
