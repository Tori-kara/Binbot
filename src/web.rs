use axum::{
    response::{Html, Redirect},
    routing::get,
    Json, Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

struct AppState {
    discord_bot_url: Option<String>,
}

/// Starts a lightweight background HTTP server for health checks & bot invitation portal.
/// Render Web Services ping this endpoint to verify container health.
pub async fn start_health_server(port: u16, discord_bot_url: Option<String>) {
    let state = Arc::new(AppState {
        discord_bot_url: discord_bot_url.clone(),
    });

    let state_for_root = state.clone();
    let state_for_healthz = state.clone();
    let state_for_invite = state.clone();

    let app = Router::new()
        .route(
            "/",
            get(move || {
                let state = state_for_root.clone();
                async move { Html(render_landing_page(state.discord_bot_url.as_deref())) }
            }),
        )
        .route(
            "/healthz",
            get(move || {
                let state = state_for_healthz.clone();
                async move {
                    Json(json!({
                        "status": "ok",
                        "app": "binbot",
                        "discord_bot_url": state.discord_bot_url
                    }))
                }
            }),
        )
        .route(
            "/invite",
            get(move || {
                let state = state_for_invite.clone();
                async move {
                    if let Some(ref url) = state.discord_bot_url {
                        Redirect::temporary(url)
                    } else {
                        Redirect::temporary("/")
                    }
                }
            }),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => {
            tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("Health server terminated: {e}");
                }
            });
            tracing::info!("✓ Web health & invite portal running on port {port}");
        }
        Err(e) => {
            tracing::warn!("Health server could not bind to port {port}: {e}");
        }
    }
}

fn render_landing_page(bot_url: Option<&str>) -> String {
    let invite_href = bot_url.unwrap_or("#");
    let button_disabled_attr = if bot_url.is_none() { "disabled" } else { "" };
    let button_text = if bot_url.is_some() {
        "🤖 Add Binbot to Discord Server"
    } else {
        "⚠️ Invite Link Pending Configuration"
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Binbot — Crypto & Multi-Fiat Discord Bot Engine</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&display=swap" rel="stylesheet">
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: 'Inter', system-ui, -apple-system, sans-serif;
            background-color: #0b0e14;
            color: #f0f4f8;
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
            padding: 1.5rem;
            background-image: 
                radial-gradient(circle at 15% 20%, rgba(88, 101, 242, 0.15) 0%, transparent 40%),
                radial-gradient(circle at 85% 80%, rgba(240, 185, 11, 0.12) 0%, transparent 40%);
        }}
        .container {{
            max-width: 640px;
            width: 100%;
            background: rgba(22, 27, 34, 0.75);
            border: 1px solid rgba(255, 255, 255, 0.1);
            backdrop-filter: blur(16px);
            border-radius: 20px;
            padding: 2.5rem;
            box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5);
            text-align: center;
        }}
        .badge {{
            display: inline-flex;
            align-items: center;
            gap: 8px;
            background: rgba(35, 134, 54, 0.2);
            border: 1px solid rgba(46, 160, 67, 0.4);
            color: #3fb950;
            padding: 6px 14px;
            border-radius: 30px;
            font-size: 0.85rem;
            font-weight: 600;
            margin-bottom: 1.5rem;
        }}
        .dot {{
            width: 8px;
            height: 8px;
            background-color: #3fb950;
            border-radius: 50%;
            box-shadow: 0 0 10px #3fb950;
        }}
        h1 {{
            font-size: 2.2rem;
            font-weight: 800;
            letter-spacing: -0.02em;
            background: linear-gradient(135deg, #ffffff 0%, #cbd5e1 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
            margin-bottom: 0.75rem;
        }}
        p.subtitle {{
            color: #94a3b8;
            font-size: 1.05rem;
            line-height: 1.6;
            margin-bottom: 2rem;
        }}
        .cta-btn {{
            display: inline-flex;
            align-items: center;
            justify-content: center;
            gap: 12px;
            width: 100%;
            padding: 1rem 1.75rem;
            background: linear-gradient(135deg, #5865F2 0%, #4752C4 100%);
            color: #ffffff;
            font-size: 1.05rem;
            font-weight: 700;
            text-decoration: none;
            border-radius: 12px;
            transition: all 0.25s ease;
            box-shadow: 0 8px 24px rgba(88, 101, 242, 0.35);
            border: none;
            cursor: pointer;
        }}
        .cta-btn:hover:not([disabled]) {{
            transform: translateY(-2px);
            box-shadow: 0 12px 30px rgba(88, 101, 242, 0.5);
            background: linear-gradient(135deg, #4752C4 0%, #3c45a5 100%);
        }}
        .cta-btn[disabled] {{
            opacity: 0.6;
            cursor: not-allowed;
            background: #334155;
            box-shadow: none;
        }}
        .features {{
            display: grid;
            grid-template-columns: repeat(3, 1fr);
            gap: 12px;
            margin-top: 2rem;
            padding-top: 2rem;
            border-top: 1px solid rgba(255, 255, 255, 0.08);
        }}
        .feature-card {{
            background: rgba(255, 255, 255, 0.03);
            border: 1px solid rgba(255, 255, 255, 0.05);
            padding: 0.9rem 0.6rem;
            border-radius: 10px;
            font-size: 0.85rem;
            color: #cbd5e1;
        }}
        .feature-card strong {{
            display: block;
            color: #f8fafc;
            margin-bottom: 4px;
            font-size: 0.9rem;
        }}
        .footer-links {{
            margin-top: 1.5rem;
            font-size: 0.85rem;
            color: #64748b;
        }}
        .footer-links a {{
            color: #38bdf8;
            text-decoration: none;
        }}
        .footer-links a:hover {{
            text-decoration: underline;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="badge">
            <span class="dot"></span> System Operational
        </div>
        <h1>Binbot Async Engine</h1>
        <p class="subtitle">High-performance cryptocurrency ticker ingestion, in-memory state management, 20 multi-fiat exchange conversions, and interactive Discord slash commands.</p>
        
        <a href="{invite_href}" target="_blank" class="cta-btn" {button_disabled_attr}>
            {button_text}
        </a>

        <div class="features">
            <div class="feature-card">
                <strong>⚡ &lt;3ms Response</strong>
                In-memory cached reads
            </div>
            <div class="feature-card">
                <strong>📈 Real-time Stream</strong>
                Binance WS multiplexer
            </div>
            <div class="feature-card">
                <strong>💱 20 Fiat Currencies</strong>
                Differential FX caching
            </div>
        </div>

        <div class="footer-links">
            Endpoints: <a href="/healthz">GET /healthz</a> | <a href="/invite">GET /invite</a>
        </div>
    </div>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_landing_page_with_url() {
        let url = "https://discord.com/oauth2/authorize?client_id=123&permissions=18432&scope=bot";
        let html = render_landing_page(Some(url));
        assert!(html.contains(url));
        assert!(html.contains("Add Binbot to Discord Server"));
        assert!(html.contains("class=\"cta-btn\" "));
    }

    #[test]
    fn test_render_landing_page_without_url() {
        let html = render_landing_page(None);
        assert!(html.contains("class=\"cta-btn\" disabled"));
        assert!(html.contains("Invite Link Pending Configuration"));
    }
}


