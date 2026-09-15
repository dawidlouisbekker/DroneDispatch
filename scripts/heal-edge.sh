#!/usr/bin/env bash
# Restore an edge zone's uplink and remove injected latency. Usage: scripts/heal-edge.sh sea-north
set -euo pipefail
zone="${1:?usage: $0 <zone>}"
toxiproxy="${TOXIPROXY_URL:-http://localhost:8474}"

curl -fsS -X POST "$toxiproxy/proxies/leaf-$zone" \
  -H 'Content-Type: application/json' -d '{"enabled": true}' >/dev/null
# The latency toxic only exists if add-latency.sh ran.
curl -sS -X DELETE "$toxiproxy/proxies/leaf-$zone/toxics/latency" >/dev/null || true
echo "healed $zone"
