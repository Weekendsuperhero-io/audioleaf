#!/bin/sh
# Captures container-side state at the moment AirPlay misbehaves.
# Invoke from the host: podman exec audioleaf /usr/local/bin/diag.sh > capture.txt 2>&1

set -u

section() { printf '\n=== %s ===\n' "$*"; }

section "date / uptime"
date -u +%FT%TZ
uptime || true

section "processes"
ps -ef 2>/dev/null || ps aux

section "shairport-sync alive?"
pgrep -fa shairport-sync || echo "NO shairport-sync process"
section "nqptp alive?"
pgrep -fa nqptp || echo "NO nqptp process"
section "avahi-daemon alive?"
pgrep -fa avahi-daemon || echo "NO avahi-daemon process"

section "mDNS browse (5s, _airplay._tcp + _raop._tcp)"
timeout 5 avahi-browse -tr _airplay._tcp 2>&1 || true
timeout 5 avahi-browse -tr _raop._tcp 2>&1 || true

section "mDNS resolve Nano Viz"
avahi-resolve -n "Nano Viz._airplay._tcp.local" 2>&1 || true

section "UDP listeners (mDNS 5353, PTP 319/320, AirPlay 7000)"
if command -v ss >/dev/null 2>&1; then
    ss -lun 2>/dev/null | grep -E ':(5353|319|320|7000|6001|6002)\b' || ss -lun
else
    netstat -lun 2>/dev/null | grep -E ':(5353|319|320|7000|6001|6002)\b' || netstat -lun
fi

section "ALSA loopback state"
for f in /proc/asound/Loopback/pcm*c/sub*/status \
         /proc/asound/Loopback/pcm*c/sub*/hw_params \
         /proc/asound/Loopback/pcm*p/sub*/status \
         /proc/asound/Loopback/pcm*p/sub*/hw_params; do
    [ -e "$f" ] || continue
    printf -- '--- %s ---\n' "$f"
    cat "$f" 2>&1
done

section "ALSA cards"
cat /proc/asound/cards 2>&1 || true

section "dbus system socket"
ls -la /var/run/dbus/ 2>&1 || true

section "metadata pipe"
ls -la /tmp/shairport-sync-metadata 2>&1 || true

section "done"
date -u +%FT%TZ
