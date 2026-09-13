#!/usr/bin/env bash
# ==============================================================================
# Binbot Render Automated Check & Wake-up Script (Bash / POSIX)
#
# Usage:
#   ./scripts/wakeup.sh [BACKEND_URL] [COMMAND...]
#
# Examples:
#   ./scripts/wakeup.sh https://binbot.onrender.com
#   ./scripts/wakeup.sh https://binbot.onrender.com cargo test
#   ./scripts/wakeup.sh https://binbot.onrender.com curl https://binbot.onrender.com/healthz
# ==============================================================================

set -e

BACKEND_URL="${1:-"https://binbot.onrender.com"}"
shift || true

HEALTH_URL="${BACKEND_URL%/}/healthz"

echo "🔍 Checking Render backend status at: $HEALTH_URL..."

# 1. Initial fast check (timeout 2 seconds)
HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" --max-time 2 "$HEALTH_URL" 2>/dev/null || echo "000")

if [ "$HTTP_STATUS" = "200" ]; then
    echo "⚡ [OK] Backend is active and running! Proceeding immediately."
else
    echo "😴 [SLEEPING] Backend is sleeping or starting up (HTTP Status: $HTTP_STATUS)."
    echo "🚀 Sending wake-up ping to trigger Render instance boot..."
    
    # Trigger wake-up request asynchronously/non-blocking
    curl -s "$HEALTH_URL" > /dev/null 2>&1 &
    
    MAX_RETRIES=30
    RETRY_COUNT=0
    AWAKE=false
    
    while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
        sleep 2
        RETRY_COUNT=$((RETRY_COUNT + 1))
        
        STATUS=$(curl -s -o /dev/null -w "%{http_code}" --max-time 3 "$HEALTH_URL" 2>/dev/null || echo "000")
        if [ "$STATUS" = "200" ]; then
            AWAKE=true
            echo "🟢 [READY] Render backend successfully woke up in ~$((RETRY_COUNT * 2))s!"
            break
        else
            echo "   ⏳ Waiting for Render instance... (${RETRY_COUNT}/${MAX_RETRIES})"
        fi
    done

    if [ "$AWAKE" = "false" ]; then
        echo "❌ [TIMEOUT] Render backend did not wake up within 60 seconds."
        exit 1
    fi
fi

# 2. Execute target command if provided
if [ $# -gt 0 ]; then
    echo "▶️ Executing target command: $*"
    exec "$@"
fi
