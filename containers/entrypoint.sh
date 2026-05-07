#!/bin/sh
set -eu

PIPE="${AUDIOLEAF_SHAIRPORT_METADATA_PIPE:-/tmp/shairport-sync-metadata}"

if [ ! -p "$PIPE" ]; then
    rm -f "$PIPE"
    mkfifo "$PIPE"
    chmod 0666 "$PIPE"
fi

log() { printf '[entrypoint %s] %s\n' "$(date -u +%FT%TZ)" "$*"; }

# We rely on the HOST's dbus + avahi-daemon (bind-mounted via the compose
# file / Quadlet). Running our own here would race the host for the mDNS
# hostname and produce a "Host name conflict, retrying with HOST-N" loop.
if [ ! -S /var/run/dbus/system_bus_socket ]; then
    log "FATAL: /var/run/dbus/system_bus_socket missing — bind-mount the host's /var/run/dbus into the container"
    exit 1
fi

nqptp >&2 &
NQPTP_PID=$!
log "nqptp pid=$NQPTP_PID"

# -u: log to stderr instead of syslog. -vv: verbose (paired with diagnostics
# block in shairport-sync.conf). Drop -vv once we've captured the failure.
shairport-sync -u -vv >&2 &
SHAIRPORT_PID=$!
log "shairport-sync pid=$SHAIRPORT_PID"

cleanup() {
    log "cleanup: stopping shairport=$SHAIRPORT_PID nqptp=$NQPTP_PID"
    kill -TERM "$SHAIRPORT_PID" "$NQPTP_PID" 2>/dev/null || true
    wait "$SHAIRPORT_PID" "$NQPTP_PID" 2>/dev/null || true
}
trap cleanup TERM INT

exec audioleaf --host 0.0.0.0 --port 8787 "$@"
