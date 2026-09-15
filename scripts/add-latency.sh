#!/usr/bin/env bash
# Add latency to an edge zone's uplink. Usage: scripts/add-latency.sh sea-north 150 [jitter_ms]
set -euo pipefail
zone="${1:?usage: $0 <zone> <latency_ms> [jitter_ms]}"
latency="${2:?usage: $0 <zone> <latency_ms> [jitter_ms]}"
jitter="${3:-0}"
toxiproxy="${TOXIPROXY_URL:-http://localhost:8474}"

# Replace any previous latency toxic.
curl -sS -X DELETE "$toxiproxy/proxies/leaf-$zone/toxics/latency" >/dev/null || true
curl -fsS -X POST "$toxiproxy/proxies/leaf-$zone/toxics" \
  -H 'Content-Type: application/json' \
  -d "{\"name\": \"latency\", \"type\": \"latency\", \"attributes\": {\"latency\": $latency, \"jitter\": $jitter}}" >/dev/null
echo "added ${latency}ms (±${jitter}ms) latency to $zone"
