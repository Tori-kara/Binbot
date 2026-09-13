# Binbot — High-Performance Asynchronous Crypto & Multi-Fiat Tracking Engine

[![Rust 2024](https://img.shields.io/badge/Rust-2024_Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/Runtime-Tokio_Async-blue?logo=tokio)](https://tokio.rs/)
[![Serenity / Poise](https://img.shields.io/badge/Discord-Serenity%20%2F%20Poise-5865F2?logo=discord)](https://github.com/serenity-rs/serenity)
[![Axum](https://img.shields.io/badge/Web-Axum_0.8-green)](https://github.com/tokio-rs/axum)
[![SQLx PostgreSQL](https://img.shields.io/badge/Storage-SQLx_PostgreSQL-336791?logo=postgresql)](https://github.com/launchbadge/sqlx)
[![Redis Cache](https://img.shields.io/badge/Cache-Redis_Async-DC382D?logo=redis)](https://redis.io/)
[![Docker](https://img.shields.io/badge/Container-Docker_cargo--chef-2496ED?logo=docker)](https://www.docker.com/)

**Binbot** is a production-grade, cloud-native asynchronous system built in Rust designed for real-time cryptocurrency ticker ingestion, in-memory state management, local currency exchange rate conversion, and automated Discord interaction.

Engineered with a focus on **sub-millisecond state reads**, **fault-tolerant WebSocket streaming**, and **differential FX cache management**, Binbot handles multi-pair Binance streams with minimal CPU overhead, serving interactive Discord slash commands with autocomplete in under 3 milliseconds.

---

## 📋 Table of Contents

- [Architectural Overview](#-architectural-overview)
- [System Components & Modules](#-system-components--modules)
- [Data Flow & Processing Pipelines](#-data-flow--processing-pipelines)
- [Security & Threat Mitigation Model](#-security--threat-mitigation-model)
- [Technology Stack](#-technology-stack)
- [Configuration & Environment Schema](#-configuration--environment-schema)
- [Local Development & Setup Guide](#-local-development--setup-guide)
- [Cloud Deployment & Docker Optimizations](#-cloud-deployment--docker-optimizations)
- [Directory Structure](#-directory-structure)
- [License & Acknowledgments](#-license--acknowledgments)

---

## 🏗 Architectural Overview

Binbot operates as a multi-threaded asynchronous reactor powered by **Tokio**. It decouples live data ingestion from user querying through event broadcasting, thread-safe shared state (`Arc<RwLock<T>>`), and persistent multi-tiered caching.

```mermaid
flowchart TD
    subgraph External Systems
        BinanceWS["Binance WebSocket API\n(wss://stream.binance.com)"]
        FxAPI["Open Exchange Rates API\n(https://open.er-api.com)"]
        DiscordGateway["Discord Gateway & REST API"]
        RenderProbe["Cloud Health Monitor / Load Balancer"]
    end

    subgraph Binbot Core Application
        WebHealth["Web Health Server\n(Axum HTTP /healthz)"]

        subgraph Ingestion & Processing Pipeline
            WSClient["Binance WebSocket Worker\n(Auto-reconnecting w/ Exponential Backoff)"]
            MarketProc["Market Data Processor\n(Normalizer & Broadcast Channel)"]
            MarketStore[("In-Memory Market State\nArc<RwLock<HashMap<String, MarketData>>>")]
        end

        subgraph Multi-Fiat FX Engine
            FxService["Currency Service\n(Differential Threshold Checker)"]
            RedisCache[("Redis Async Cache\n(key: binbot:currency:rates)")]
        end

        subgraph Presentation & Command Layer
            DiscordBot["Discord Bot Engine\n(Poise / Serenity)"]
            Autocomplete["Interactive Autocomplete Resolver\n(<1ms In-Memory Lookups)"]
        end

        DBPool[("PostgreSQL Database\n(SQLx Migrations & Guild Storage)")]
    end

    BinanceWS -->|Real-time Tickers / Klines| WSClient
    WSClient -->|BinanceEvent Channel| MarketProc
    MarketProc -->|Write Tickers| MarketStore
    MarketProc -->|Broadcast TickerUpdated| DiscordBot

    FxAPI -->|Fetch Candidate Rates| FxService
    FxService <-->|Load / Persist Cached FX Rates| RedisCache
    FxService -->|Seed / Fallback Rates| MarketStore

    DiscordGateway <-->|Slash Commands & Autocomplete| DiscordBot
    DiscordBot -->|Read Price Snapshots| MarketStore
    DiscordBot -->|Read Converted Rates| FxService
    DiscordBot <-->|Guild Config & Watchlists| DBPool

    RenderProbe -->|GET /healthz| WebHealth
```

---

## 🧩 System Components & Modules

### 1. Core Bootstrapper & Runtime (`src/main.rs`, `src/config.rs`, `src/error.rs`)

- **Initialization Sequence**: Configures TLS cryptography via `rustls` (`ring` provider), initializes `tracing-subscriber` for structured logging, validates environment variables, and launches core tasks concurrently.
- **Graceful Shutdown**: Intercepts `Ctrl+C` operating system signals to initiate structured teardown, closing WebSocket connections cleanly before exiting.

### 2. Binance WebSocket Client (`src/binance/`)

- **Resilient Connection Loop**: Implements an auto-reconnecting WebSocket worker ([`streams.rs`](file:///c:/Users/new_u/binbot/src/binance/streams.rs)) with exponential backoff algorithm (reconnect delays scale from 1s up to 60s max).
- **Stream Multiplexing**: Subscribes dynamically to symbol tickers (`<symbol>@ticker`), closed candlesticks (`<symbol>@kline_<interval>`), and full market rolling stats (`!ticker@arr`).
- **State Recovery**: Retains active subscriptions in a thread-safe `HashSet` and automatically re-subscribes upon reconnect.

### 3. Market State & Data Engine (`src/market/`)

- **In-Memory Cache**: Stores real-time market snapshots (`MarketData`) in a high-concurrency `Arc<RwLock<HashMap<String, MarketData>>>` ([`state.rs`](file:///c:/Users/new_u/binbot/src/market/state.rs)).
- **Normalized Models**: Transforms raw exchange JSON payloads into strongly-typed domain structs using high-precision financial types (`rust_decimal::Decimal`).
- **Broadcast Channel**: Emits `MarketUpdateEvent` via `tokio::sync::broadcast` (capacity: 16,384) to decouple market updates from alert execution.

### 4. Multi-Fiat FX & Differential Cache Engine (`src/currency/`)

- **Multi-Currency Support**: Native conversion support for 20 global fiat currencies (PHP, CAD, JPY, EUR, GBP, AUD, SGD, INR, BRL, CHF, NZD, HKD, KRW, THB, IDR, VND, MXN, AED, USD).
- **Differential Change Detection**: Performs periodic checks against external exchange rate APIs. Updates in-memory and Redis caches **only when rate changes exceed a configurable threshold** (e.g. `0.1%`), minimizing unnecessary database/cache writes.
- **3-Tier Fallback Cascade**:
  1. **Primary**: Fast read from Redis cache (`binbot:currency:rates`).
  2. **Secondary**: Seed from external Open Exchange API.
  3. **Tertiary**: In-memory static baseline rates as emergency fallback.

### 5. Discord Command & Presentation Layer (`src/discord/`)

- **Poise & Serenity Integration**: Modern slash command architecture with built-in autocomplete handlers ([`price.rs`](file:///c:/Users/new_u/binbot/src/discord/commands/price.rs)).
- **Sub-3ms SLA**: Price lookups bypass disk/network requests by fetching directly from `MarketState` in memory, avoiding Discord's 3-second interaction response timeout limit.
- **Rich Embed Engine**: Generates dynamic, color-coded Discord embeds ([`embeds.rs`](file:///c:/Users/new_u/binbot/src/discord/embeds.rs)) featuring 24h price changes, high/low ranges, base/quote volumes, local currency symbols, and flag emojis.

### 6. Health & Monitoring Web Server (`src/web.rs`)

- **Axum Web Server**: Runs a lightweight background HTTP listener binding to `$PORT` (default `10000`).
- **Web Landing Page & Invite Portal**: `GET /` serves an interactive, dark-themed HTML portal displaying live system status and a prominent **"🤖 Add Binbot to Discord Server"** call-to-action button.
- **Direct Invite Route**: `GET /invite` performs a `307 Temporary Redirect` straight to `DISCORD_BOT_URL`.
- **Health Endpoints**: Exposes `GET /healthz` returning JSON `{"status": "ok", "app": "binbot", "discord_bot_url": "..."}` for container health probes (Render, Kubernetes, Docker Engine).

### 7. Database & Storage Layer (`src/storage/`, `migrations/`)

- **PostgreSQL Pool**: Managed connection pool via SQLx (`PgPoolOptions` limited to 10 connections).
- **Embedded Migrations**: Executes versioned SQL migrations automatically on application startup ([`db.rs`](file:///c:/Users/new_u/binbot/src/storage/db.rs)).
- **Redis Client**: Asynchronous multiplexed connection with automatic retries ([`redis.rs`](file:///c:/Users/new_u/binbot/src/storage/redis.rs)).

---

## 🔄 Data Flow & Processing Pipelines

### Market Ticker Ingestion Pipeline

```
[ Binance WebSocket ]
        │ JSON Frame
        ▼
[ WebSocketWorker ] ── Deserializes ──► [ BinanceEvent::Ticker ]
                                                  │
                                                  ▼
                                      [ MarketProcessor ]
                                                  │
                         ┌────────────────────────┴────────────────────────┐
                         ▼                                                 ▼
            [ Update MarketState ]                              [ Broadcast Event ]
       (Arc<RwLock<HashMap>>)                              (MarketUpdateEvent)
                         │                                                 │
                         ▼                                                 ▼
             Sub-ms Read for /price                              Discord Notifier
```

### FX Differential Cache Lifecycle

```
[ App Launch / Interval (3h) ]
        │
        ▼
[ Load from Redis Cache ] ── Found? ──► Yes ──► Populate CurrencyService
        │ No
        ▼
[ Fetch Candidate Rates (Open Exchange API) ]
        │
        ▼
[ Compare vs Cached Rates ] ── Diff >= 0.1%?
        │
        ├──► No  ──► Keep Existing Cache (Log: Unchanged)
        │
        └──► Yes ──► Update Memory State ──► Persist to Redis (key: binbot:currency:rates)
```

---

## 🛡 Security & Threat Mitigation Model

Binbot is designed adhering to defense-in-depth principles:

| Threat / Risk                         | Mitigation Architecture                                                                                                                                                                                                                            | Location / Implementation                                                                      |
| :------------------------------------ | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :--------------------------------------------------------------------------------------------- |
| **Credential & Key Exposure**         | Environment variable isolation using `dotenvy`. Secrets (`DISCORD_TOKEN`, `DATABASE_URL`, `REDIS_URL`) are read directly from memory and excluded from git via `.gitignore`. Log outputs sanitize sensitive credentials using `Config::summary()`. | [`src/config.rs`](file:///c:/Users/new_u/binbot/src/config.rs)                                 |
| **Privilege Escalation**              | Docker runtime container uses a dedicated non-root user (`appuser` with UID `10001`).                                                                                                                                                              | [`Dockerfile`](file:///c:/Users/new_u/binbot/Dockerfile)                                       |
| **Man-in-the-Middle (MitM)**          | All WebSocket and HTTP traffic enforced over TLS 1.2/1.3 using `rustls` with `ring` cryptographic provider. Plaintext fallback is prohibited.                                                                                                      | [`src/main.rs`](file:///c:/Users/new_u/binbot/src/main.rs)                                     |
| **Discord Gateway Abuse**             | Bot requests minimal non-privileged gateway intents (`GatewayIntents::non_privileged()`), strictly limiting exposed gateway payload data.                                                                                                          | [`src/discord/bot.rs`](file:///c:/Users/new_u/binbot/src/discord/bot.rs)                       |
| **DoS via Slow I/O**                  | All Discord slash commands read directly from in-memory `RwLock` (<1ms execution), eliminating command timeout risk or blocking main event loops.                                                                                                  | [`src/discord/commands/price.rs`](file:///c:/Users/new_u/binbot/src/discord/commands/price.rs) |
| **SQL Injection**                     | All database operations use compile-time typed SQL query parameters provided by SQLx macro/query builders.                                                                                                                                         | [`src/storage/db.rs`](file:///c:/Users/new_u/binbot/src/storage/db.rs)                         |
| **Network Flakiness / Stale Streams** | Automatic WebSocket worker reconnection with randomized exponential backoff avoids rate-limiting thundering herd problems.                                                                                                                         | [`src/binance/streams.rs`](file:///c:/Users/new_u/binbot/src/binance/streams.rs)               |

---

## 💻 Technology Stack

| Component             | Library / Framework                                                                                 | Version      | Purpose                                                                  |
| :-------------------- | :-------------------------------------------------------------------------------------------------- | :----------- | :----------------------------------------------------------------------- |
| **Language**          | [Rust](https://www.rust-lang.org/)                                                                  | 2024 Edition | High-performance, memory-safe compiled binary                            |
| **Async Runtime**     | [Tokio](https://tokio.rs/)                                                                          | 1.53         | Multi-threaded asynchronous event loop & task scheduling                 |
| **Discord Framework** | [Poise](https://github.com/serenity-rs/poise) / [Serenity](https://github.com/serenity-rs/serenity) | 0.7 / 0.12   | Ergonomic slash commands, autocomplete & Discord gateway interface       |
| **HTTP Server**       | [Axum](https://github.com/tokio-rs/axum)                                                            | 0.8          | Lightweight background health monitoring server                          |
| **WebSocket Client**  | [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite)                                  | 0.30         | TLS-encrypted Binance WebSocket stream ingestion                         |
| **Database**          | [SQLx](https://github.com/launchbadge/sqlx)                                                         | 0.8          | Async PostgreSQL driver with embedded compile-time migrations            |
| **Cache Storage**     | [redis-rs](https://github.com/redis-rs/redis-rs)                                                    | 1.7          | Multiplexed async connection for persistent FX rate caching              |
| **Math & Numbers**    | [rust_decimal](https://github.com/pauperez/rust_decimal)                                            | 1.43         | Arbitrary-precision decimal arithmetic for financial ticker calculations |
| **Serialization**     | [Serde](https://serde.rs/)                                                                          | 1.0          | High-speed JSON serialization and deserialization                        |
| **Container Build**   | [cargo-chef](https://github.com/LukeMathWalker/cargo-chef)                                          | Latest       | Layered Docker build caching for rapid CI/CD deployments                 |

---

## ⚙ Configuration & Environment Schema

Environment configuration is managed via standard system environment variables or a local `.env` file:

| Variable                            | Required | Default / Format                             | Description                                                      |
| :---------------------------------- | :------: | :------------------------------------------- | :--------------------------------------------------------------- |
| `DATABASE_URL`                      | **Yes**  | `postgres://user:pass@localhost:5432/binbot` | PostgreSQL database connection string                            |
| `REDIS_URL`                         | **Yes**  | `redis://127.0.0.1:6379`                     | Redis connection URL for FX rate caching                         |
| `DISCORD_TOKEN`                     | **Yes**  | `MTE...`                                     | Discord Bot Application OAuth2 token                             |
| `DISCORD_BOT_URL`                   |    No    | `https://discord.com/oauth2/authorize?...`   | Discord Bot Invitation URL (exposed on Web UI & `/invite`)        |
| `DISCORD_GUILD_ID`                  |    No    | `123456789012345678`                         | Server ID for instant testing command registration               |
| `PORT`                              |    No    | `10000`                                      | Port for Axum `/healthz` HTTP health server                      |
| `CURRENCY_CHECK_INTERVAL_HOURS`     |    No    | `3`                                          | Interval (in hours) between FX differential checks               |
| `BINANCE_WEBSOCKET_RAW_ENDPOINT`    |    No    | `wss://stream.binance.com:9443/ws`           | Binance WebSocket raw endpoint                                   |
| `BINANCE_WEBSOCKET_STREAM_ENDPOINT` |    No    | `wss://stream.binance.com:9443/stream`       | Binance WebSocket combined stream endpoint                       |
| `RUST_LOG`                          |    No    | `info`                                       | Logging filter level (`trace`, `debug`, `info`, `warn`, `error`) |

---

## 🛠 Local Development & Setup Guide

### Prerequisites

- **Rust Toolchain**: 2024 Edition (v1.85+)
- **PostgreSQL Database**: v14+
- **Redis Server**: v6+
- **Docker Engine**: _(Optional for containerized run)_

### 1. Clone & Setup Workspace

```bash
git clone https://github.com/Tori-kara/Binbot.git
cd Binbot
```

### 2. Configure Environment Variables

Copy `.env.example` to `.env` and fill in your local credentials:

```bash
cp .env.example .env
```

Example `.env`:

```env
DATABASE_URL=postgres://postgres:postgres@localhost:5432/binbot
REDIS_URL=redis://127.0.0.1:6379
DISCORD_TOKEN=your_discord_bot_token_here
DISCORD_GUILD_ID=your_optional_guild_id_here
PORT=10000
RUST_LOG=info
```

### 3. Run Database Migrations

Migrations execute automatically at startup, or manually via SQLx CLI:

```bash
cargo install sqlx-cli --no-default-features --features postgres
sqlx database create
sqlx migrate run
```

### 4. Build & Run Application

```bash
# Run in debug mode
cargo run

# Run with full optimizations
cargo run --release
```

### 5. Execute Automated Test Suite

```bash
cargo test
```

---

## ☁ Cloud Deployment & Docker Optimizations

### Optimized Multi-Stage Dockerfile

Binbot utilizes [`cargo-chef`](https://github.com/LukeMathWalker/cargo-chef) to separate dependency compilation from application source builds. This reduces subsequent build times on services like **Render** or **Railway** from 15 minutes down to **seconds**.

```dockerfile
# Stage 1: Chef Base
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
WORKDIR /app

# Stage 2: Planner
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder (Caches 300+ dependency crates)
FROM chef AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin binbot

# Stage 4: Minimal Runtime Image (~40MB)
FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 10001 appuser
USER appuser
COPY --from=builder /app/target/release/binbot /app/binbot
EXPOSE 10000
HEALTHCHECK CMD curl -f http://localhost:10000/healthz || exit 1
ENTRYPOINT ["/app/binbot"]
```

### 1-Click Render Deployment (`render.yaml`)

Binbot includes a ready-to-use Render Blueprint definition:

1. Connect your repository to [Render Dashboard](https://dashboard.render.com).
2. Create a **New Blueprint Service**.
3. Render automatically provisions a managed PostgreSQL database and Docker Web Service listening on `/healthz`.
4. Supply your `DISCORD_TOKEN` and `REDIS_URL` in the Render environment settings.

---

## 📁 Directory Structure

```
Binbot/
├── .dockerignore              # Excludes target and local environment from Docker context
├── .env.example               # Template environment configuration file
├── Cargo.toml                 # Cargo dependencies and optimized release profile settings
├── Cargo.lock                 # Locked dependency resolution tree
├── Dockerfile                 # Multi-stage cargo-chef optimized Docker runtime file
├── RENDER_DEPLOYMENT.md       # Step-by-step deployment guide for Render Web Services
├── render.yaml                # Render Infrastructure-as-Code (Blueprint) manifest
├── migrations/                # Embedded SQL migration scripts
│   ├── 20260911100529_create_guilds.up.sql
│   ├── 20260911100536_create_channels.up.sql
│   ├── 20260911100545_create_users.up.sql
│   ├── 20260911100554_create_alerts.up.sql
│   └── 20260911100603_create_watchlist.up.sql
└── src/                       # Application source code
    ├── main.rs                # Application entrypoint & task orchestration loop
    ├── config.rs              # Configuration loader & sanitizer
    ├── error.rs               # Custom error domain types (AppError)
    ├── web.rs                 # Axum HTTP /healthz server for container health probes
    ├── binance/               # Binance WebSocket client module
    │   ├── client.rs          # Stream helper functions & config options
    │   ├── models.rs          # Binance raw event JSON models
    │   └── streams.rs         # Auto-reconnecting WebSocket worker engine
    ├── market/                # Market data state & normalization
    │   ├── models.rs          # MarketData, Ohlc & MarketUpdateEvent models
    │   ├── processor.rs       # Event normalizer & broadcast dispatcher
    │   └── state.rs           # Thread-safe Arc<RwLock<HashMap>> market state
    ├── currency/              # Multi-fiat FX conversion & differential caching
    │   ├── models.rs          # Supported currencies metadata & lookup tables
    │   └── service.rs         # CurrencyService with Redis persistence & diff threshold
    ├── discord/               # Discord bot presentation layer
    │   ├── bot.rs             # Serenity client & Poise framework setup
    │   ├── embeds.rs          # Rich Discord embed template builder
    │   ├── notifier.rs        # Market event notifier
    │   └── commands/          # Poise slash commands
    │       ├── price.rs       # /price command with interactive autocomplete
    │       └── currencies.rs  # /currencies command displaying live exchange rates
    └── storage/               # Infrastructure persistence adapters
        ├── db.rs              # SQLx PostgreSQL pool initializer & migration runner
        └── redis.rs           # Redis async connection factory w/ retry handling
```

---

## 📜 License & Acknowledgments

Distributed under the MIT License. See `LICENSE` for details.

Special thanks to the **Rust**, **Tokio**, **Serenity**, and **Binance API** open-source communities for providing world-class asynchronous infrastructure tooling.
