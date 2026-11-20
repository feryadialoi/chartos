#!/bin/sh
# Creates a Viewer service account in the local Grafana and prints a token for chartos.
set -eu
GRAFANA_URL="${GRAFANA_URL:-http://localhost:3000}"
AUTH="admin:admin"

id=$(curl -sf -u "$AUTH" -H 'Content-Type: application/json' \
  -d '{"name":"chartos","role":"Viewer"}' "$GRAFANA_URL/api/serviceaccounts" | sed -E 's/.*"id":([0-9]+).*/\1/') \
  || id=$(curl -sf -u "$AUTH" "$GRAFANA_URL/api/serviceaccounts/search?query=chartos" | sed -E 's/.*"id":([0-9]+).*/\1/')

curl -sf -u "$AUTH" -H 'Content-Type: application/json' \
  -d "{\"name\":\"chartos-$(date +%s)\"}" "$GRAFANA_URL/api/serviceaccounts/$id/tokens" \
  | sed -E 's/.*"key":"([^"]+)".*/\1/'
