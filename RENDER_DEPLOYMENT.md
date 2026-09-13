# Deploying Binbot on Render (Web Service)

This guide walks you through deploying **Binbot** on [Render](https://render.com) using a Docker Web Service.

---

## Architecture Overview

- **Service Type**: **Web Service** (Docker runtime)
- **Health Check Path**: `/healthz` (serves `{"status": "ok", "app": "binbot"}`)
- **Build Optimization**: Multi-stage `cargo-chef` caching layers dependencies, cutting subsequent rebuilds from 15 minutes down to seconds, preventing Render build timeouts.
- **Managed Resources**:
  - PostgreSQL (can use Render PostgreSQL or external like Supabase/Neon)
  - Redis (can use Render Key-Value/Redis or Upstash Redis)

---

## Deployment Option A: 1-Click Render Blueprint (Recommended)

1. **Push your code to GitHub / GitLab**.
2. Go to the [Render Dashboard](https://dashboard.render.com).
3. Click **New +** → **Blueprint**.
4. Connect your Binbot repository. Render will automatically detect [`render.yaml`](./render.yaml).
5. Render will automatically:
   - Create a free PostgreSQL database (`binbot-postgres`).
   - Create the `binbot` Web Service.
   - Wire `DATABASE_URL` directly from the database connection string.
6. Fill in the required environment variables:
   - `DISCORD_TOKEN`: Your Discord Bot Token.
   - `REDIS_URL`: Your Redis connection string (`redis://...` or `rediss://...`).
   - *(Optional)* `DISCORD_GUILD_ID`: Server ID for instant command registration during testing.
7. Click **Apply**.

---

## Deployment Option B: Manual Web Service Setup

If you prefer manual setup without Blueprints:

1. In Render Dashboard, click **New +** → **Web Service**.
2. Connect your Git repository.
3. Configure the service:
   - **Name**: `binbot`
   - **Language**: `Docker`
   - **Dockerfile Path**: `./Dockerfile`
   - **Docker Context**: `.`
   - **Plan**: `Free` (or `Starter`)
4. In **Advanced** settings:
   - **Health Check Path**: `/healthz`
5. Add the following **Environment Variables**:

| Variable | Required | Example / Default | Description |
| :--- | :--- | :--- | :--- |
| `DATABASE_URL` | **Yes** | `postgres://user:pass@host/binbot` | PostgreSQL connection string |
| `REDIS_URL` | **Yes** | `redis://default:pass@host:6379` | Redis connection string |
| `DISCORD_TOKEN` | **Yes** | `MTE...` | Discord bot application token |
| `DISCORD_BOT_URL` | No | `https://discord.com/oauth2/authorize?...` | Discord bot invitation link (shown on web root UI & `/invite`) |
| `DISCORD_GUILD_ID` | No | `123456789012345678` | Server ID for guild-scoped commands |
| `PORT` | Auto | `10000` | Port for healthcheck (injected by Render) |
| `RUST_LOG` | No | `info` | Logging verbosity |
| `BINANCE_WEBSOCKET_RAW_ENDPOINT` | No | `wss://stream.binance.com:9443/ws` | Binance WebSocket raw endpoint |
| `BINANCE_WEBSOCKET_STREAM_ENDPOINT`| No | `wss://stream.binance.com:9443/stream` | Binance combined stream endpoint |

6. Click **Create Web Service**.

---

## How Build Timeouts are Prevented

Rust release builds with high dependency counts can hit Render's 15-minute build limits or run out of memory. Binbot prevents this through:

1. **`cargo-chef` Layer Caching**:
   Dependencies are cooked into a separate Docker layer (`recipe.json`). Whenever you push application code changes, all 300+ dependencies are retrieved instantly from cache.
2. **Optimized Release Profile**:
   Configured in `Cargo.toml`:
   - `lto = "thin"`: Avoids the high memory overhead and slow link times of full LTO.
   - `codegen-units = 16`: Ensures parallel compilation across available CPU cores.
   - `strip = true`: Strips debug symbols to keep the output artifact small and minimize disk writes.
   - `panic = "abort"`: Eliminates unwinding landing pads for a smaller binary.

---

## How the Web Server & Health Check Work

Render Web Services require listening on the `$PORT` environment variable (default `10000`).
Binbot runs an internal lightweight Axum web server on `$PORT`:
- `GET /` → `200 OK` (Serves an interactive dark-themed landing page with system status & **Discord Bot Invitation** CTA button)
- `GET /invite` → `307 Temporary Redirect` (Redirects directly to `DISCORD_BOT_URL` so server owners can invite the bot via a clean link)
- `GET /healthz` → `200 OK` (Returns JSON `{"status": "ok", "app": "binbot", "discord_bot_url": "..."}`)

This ensures Render's health monitor marks the deployment green immediately upon launch while providing a public-facing portal for guild installation.
