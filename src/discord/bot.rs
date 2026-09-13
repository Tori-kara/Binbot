use std::sync::Arc;
use serenity::all::{GatewayIntents, GuildId};

use crate::currency::CurrencyService;
use crate::market::MarketState;

/// Application data shared across Poise commands
#[derive(Debug)]
pub struct Data {
    pub market_state: MarketState,
    pub currency_service: Arc<CurrencyService>,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Starts the Discord bot using Poise and Serenity
pub async fn run_bot(
    token: String,
    market_state: MarketState,
    currency_service: Arc<CurrencyService>,
    guild_id: Option<u64>,
) -> Result<(), Error> {
    let options = poise::FrameworkOptions {
        commands: vec![
            crate::discord::commands::price(),
            crate::discord::commands::currencies(),
            crate::discord::commands::wakeup(),
        ],
        on_error: |error| {
            Box::pin(async move {
                tracing::error!("Discord bot error: {:?}", error);
            })
        },
        pre_command: |ctx| {
            Box::pin(async move {
                tracing::info!(
                    "Executing command '/{}' invoked by {}",
                    ctx.command().name,
                    ctx.author().name
                );
            })
        },
        post_command: |ctx| {
            Box::pin(async move {
                tracing::info!("Successfully executed command '/{}'", ctx.command().name);
            })
        },
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .options(options)
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                if let Some(gid) = guild_id {
                    tracing::info!("Registering slash commands for guild ID: {}", gid);
                    if let Err(e) = poise::builtins::register_in_guild(
                        ctx,
                        &framework.options().commands,
                        GuildId::new(gid),
                    )
                    .await
                    {
                        tracing::warn!(
                            "Failed to register slash commands in guild {}: {:?}. Ensure the bot has joined this server and was invited with the 'applications.commands' OAuth2 scope.",
                            gid,
                            e
                        );
                    } else {
                        tracing::info!("✓ Slash commands registered in guild {}", gid);
                    }
                }

                tracing::info!("Registering global slash commands...");
                if let Err(e) = poise::builtins::register_globally(ctx, &framework.options().commands).await {
                    tracing::warn!("Failed to register global slash commands: {:?}", e);
                } else {
                    tracing::info!("✓ Global slash commands registered successfully");
                }

                Ok(Data {
                    market_state,
                    currency_service,
                })
            })
        })
        .build();

    let intents = GatewayIntents::non_privileged();
    let mut client = serenity::all::ClientBuilder::new(token, intents)
        .framework(framework)
        .await?;

    tracing::info!("✓ Discord Gateway client connecting...");
    client.start().await?;

    Ok(())
}
