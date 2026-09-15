#!/usr/bin/env bash
# Cut an edge zone's uplink to the NATS hub. Usage: scripts/partition-edge.sh sea-north
set -euo pipefail
zone="${1:?usage: $0 <zone>}"
toxiproxy="${TOXIPROXY_URL:-http://localhost:8474}"

curl -fsS -X POST "$toxiproxy/proxies/leaf-$zone" \
  -H 'Content-Type: application/json' -d '{"enabled": false}' >/dev/null
echo "partitioned $zone"
