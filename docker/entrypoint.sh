#!/bin/sh
set -eu

PIPE="${AUDIOLEAF_SHAIRPORT_METADATA_PIPE:-/tmp/shairport-sync-metadata}"

if [ ! -p "$PIPE" ]; then
    rm -f "$PIPE"
    mkfifo "$PIPE"
    chmod 0666 "$PIPE"
fi

mkdir -p /var/run/dbus
rm -f /var/run/dbus/pid
dbus-daemon --system --fork

avahi-daemon --no-drop-root --no-rlimits --daemonize

nqptp &
NQPTP_PID=$!

shairport-sync &
SHAIRPORT_PID=$!

cleanup() {
    kill -TERM "$SHAIRPORT_PID" "$NQPTP_PID" 2>/dev/null || true
    wait "$SHAIRPORT_PID" "$NQPTP_PID" 2>/dev/null || true
}
trap cleanup TERM INT

exec audioleaf --host 0.0.0.0 --port 8787 "$@"
