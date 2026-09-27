#!/usr/bin/env bash
# Build the app and PUT it as the bundle of brickos/inventory@0.0.1-dev.
# The dev site pins that version, so it is live at
# http://dev.inventory.brickos.localhost:3000/ as soon as this returns.
#
#   LOCO_USER=alice LOCO_PASSWORD=password npm run deploy
#
# LOCO_ORIGIN (default http://localhost:3000) and LOCO_VERSION
# (default 0.0.1-dev) override the target.
set -euo pipefail

cd "$(dirname "$0")/.."

origin="${LOCO_ORIGIN:-http://localhost:3000}"
version="${LOCO_VERSION:-0.0.1-dev}"
: "${LOCO_USER:?set LOCO_USER to a developer on brickos/inventory}"
: "${LOCO_PASSWORD:?set LOCO_PASSWORD}"

VITE_LOCO_VERSION="$version" npx vite build

zip="$(mktemp -d)/bundle.zip"
trap 'rm -rf "$(dirname "$zip")"' EXIT
(cd dist && zip -qr "$zip" .)

login="$(jq -n --arg u "$LOCO_USER" --arg p "$LOCO_PASSWORD" '{username: $u, password: $p}')"
token="$(curl -sf "$origin/auth/login" -H 'Content-Type: application/json' -d "$login" | jq -r .data.token)"

curl -sS --fail-with-body -X PUT "$origin/schema/brickos/inventory/$version/bundle" \
  -H "Authorization: Bearer $token" \
  -H 'Content-Type: application/zip' \
  --data-binary "@$zip" | jq .
echo "Deployed. Open http://dev.inventory.brickos.localhost:${origin##*:}/"
