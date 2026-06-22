#!/usr/bin/env bash
# List recent Kiff log entries via the kiff_logger query API.
# Usage:
#   ./scripts/kiff-logs.sh              # last 30 logs
#   ./scripts/kiff-logs.sh "level:ERROR" # last 30 ERROR logs
#   ./scripts/kiff-logs.sh "*" 50       # last 50 logs

set -euo pipefail

QUERY="${1:-*}"
LIMIT="${2:-30}"
BASE_URL="${KIFF_BASE_URL:-http://127.0.0.1:8000}"
USER="${KIFF_API_USER:-Administrator}"
PASS="${KIFF_API_PASSWORD:-admin}"

url="$BASE_URL/kiff_logger/query?q=$(printf '%s' "$QUERY" | jq -sRr @uri)&limit=$LIMIT"

echo "TIME                  | LEVEL | SERVICE                          | MESSAGE"
echo "----------------------+-------+----------------------------------+-----------------------------------------"

curl -s -u "$USER:$PASS" "$url" | \
  jq -r --arg maxMsg 60 '
    .records[] |
    [
      (.timestamp / 1000 | strftime("%Y-%m-%d %H:%M:%S")),
      .level,
      .service,
      (.message | tostring | if length > ($maxMsg | tonumber) then .[0:($maxMsg | tonumber)] + "…" else . end)
    ] | @tsv
  ' | \
  while IFS=$'\t' read -r time level service message; do
    printf "%-21s | %-5s | %-32s | %s\n" "$time" "$level" "$service" "$message"
  done
