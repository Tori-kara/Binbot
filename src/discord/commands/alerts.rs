use crate::discord::bot::{Context, Error};
use crate::discord::embeds;

/// Manage your automated price alerts
#[poise::command(
    slash_command,
    subcommands("list", "delete", "clear"),
    subcommand_required
)]
pub async fn alerts(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// List all of your active alerts
#[poise::command(slash_command)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let user_discord_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();

    let alerts = ctx.data().alert_store.get_alerts_for_user(&user_discord_id).await;
    let embed = embeds::create_alert_list_embed(&alerts, &username);

    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true)).await?;
    Ok(())
}

/// Delete an alert by its ID
#[poise::command(slash_command)]
pub async fn delete(
    ctx: Context<'_>,
    #[description = "The ID of the alert to delete (found via /alerts list)"]
    id: i64,
) -> Result<(), Error> {
    let user_discord_id = ctx.author().id.to_string();

    match ctx.data().alert_store.delete_alert(id, &user_discord_id).await {
        Ok(true) => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("✓ Alert **#{id}** has been deleted."))
                    .ephemeral(true),
            )
            .await?;
        }
        Ok(false) => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("❌ Alert **#{id}** not found or you do not have permission to delete it."))
                    .ephemeral(true),
            )
            .await?;
        }
        Err(e) => {
            tracing::error!("Failed to delete alert #{id}: {e}");
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("❌ Database error deleting alert #{id}: {e}"))
                    .ephemeral(true),
            )
            .await?;
        }
    }

    Ok(())
}

/// Delete all of your active alerts
#[poise::command(slash_command)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    let user_discord_id = ctx.author().id.to_string();
    let alerts = ctx.data().alert_store.get_alerts_for_user(&user_discord_id).await;

    if alerts.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .content("You have no active alerts to clear.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let count = alerts.len();
    for a in alerts {
        let _ = ctx.data().alert_store.delete_alert(a.id, &user_discord_id).await;
    }

    ctx.send(
        poise::CreateReply::default()
            .content(format!("✓ Cleared all **{count}** of your active alerts."))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
