#!/bin/sh
# Avvia Postamon interno se non è già acceso (da cron all'avvio e ogni 5 minuti).
cd "$(dirname "$0")" || exit 1
if ! pgrep -u "$(id -u)" -x postamon-intern >/dev/null; then
  nohup ./postamon-interno postamon-interno.toml >> interno.log 2>&1 &
fi
