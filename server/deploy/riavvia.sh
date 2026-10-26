#!/bin/sh
# Ferma e riavvia Postamon interno (dopo un aggiornamento o un cambio di configurazione).
pkill -u "$(id -u)" -x postamon-intern
sleep 1
exec "$(dirname "$0")/avvia.sh"
