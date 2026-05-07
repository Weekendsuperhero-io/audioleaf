#!/bin/sh
set -eu

PIPE="${AUDIOLEAF_SHAIRPORT_METADATA_PIPE:-/tmp/shairport-sync-metadata}"

if [ ! -p "$PIPE" ]; then
    rm -f "$PIPE"
    mkfifo "$PIPE"
    chmod 0666 "$PIPE"
fi

log() { printf '[entrypoint %s] %s\n' "$(date -u +%FT%TZ)" "$*"; }

mkdir -p /var/run/dbus
rm -f /var/run/dbus/pid
dbus-daemon --system --fork
log "dbus-daemon started"

# Avahi: foreground mode so its log goes to our stderr instead of syslog.
# --no-rlimits keeps it happy in containers without rlimit caps.
avahi-daemon --no-drop-root --no-rlimits >&2 &
AVAHI_PID=$!
log "avahi-daemon pid=$AVAHI_PID"

nqptp >&2 &
NQPTP_PID=$!
log "nqptp pid=$NQPTP_PID"

# -u: log to stderr instead of syslog. -vv: verbose (paired with diagnostics
# block in shairport-sync.conf). Drop -vv once we've captured the failure.
shairport-sync -u -vv >&2 &
SHAIRPORT_PID=$!
log "shairport-sync pid=$SHAIRPORT_PID"

cleanup() {
    log "cleanup: stopping shairport=$SHAIRPORT_PID nqptp=$NQPTP_PID avahi=$AVAHI_PID"
    kill -TERM "$SHAIRPORT_PID" "$NQPTP_PID" "$AVAHI_PID" 2>/dev/null || true
    wait "$SHAIRPORT_PID" "$NQPTP_PID" "$AVAHI_PID" 2>/dev/null || true
}
trap cleanup TERM INT

exec audioleaf --host 0.0.0.0 --port 8787 "$@"
