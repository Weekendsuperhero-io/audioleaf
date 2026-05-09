#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
CONFIG_SOURCE="${SCRIPT_DIR}/piWebServer/shairport-sync.conf"
ASOUND_SOURCE="${SCRIPT_DIR}/piWebServer/asound-default-loopback.conf"

if [[ ! -f "${CONFIG_SOURCE}" ]]; then
  echo "ERROR: Expected config file not found: ${CONFIG_SOURCE}" >&2
  exit 1
fi
if [[ ! -f "${ASOUND_SOURCE}" ]]; then
  echo "ERROR: Expected ALSA config file not found: ${ASOUND_SOURCE}" >&2
  exit 1
fi

if ! command -v sudo >/dev/null 2>&1; then
  echo "ERROR: sudo is required." >&2
  exit 1
fi

if ! command -v apt-get >/dev/null 2>&1; then
  echo "ERROR: apt-get not found. This script targets Debian/Raspberry Pi OS." >&2
  exit 1
fi

if ! command -v systemctl >/dev/null 2>&1; then
  echo "ERROR: systemctl not found. This script expects a systemd host." >&2
  exit 1
fi

echo "[0/11] Validate sudo access"
sudo -v

echo "[1/11] Stop old services"
sudo systemctl disable --now shairport-sync >/dev/null 2>&1 || true
sudo systemctl disable --now nqptp >/dev/null 2>&1 || true

echo "[2/11] Remove distro packages (if present)"
sudo apt-get remove -y shairport-sync nqptp || true
sudo apt-get autoremove -y

echo "[3/11] Remove old manual binaries/service files"
for f in \
  /usr/local/bin/shairport-sync /usr/local/sbin/shairport-sync \
  /usr/local/bin/nqptp /usr/local/sbin/nqptp \
  /etc/systemd/system/shairport-sync.service /lib/systemd/system/shairport-sync.service \
  /etc/systemd/user/shairport-sync.service /lib/systemd/user/shairport-sync.service \
  /etc/systemd/system/nqptp.service /lib/systemd/system/nqptp.service \
  /etc/systemd/user/nqptp.service /lib/systemd/user/nqptp.service \
  /etc/init.d/shairport-sync /etc/init.d/nqptp \
  /etc/dbus-1/system.d/shairport-sync-dbus.conf /etc/dbus-1/system.d/shairport-sync-mpris.conf
do
  sudo rm -f "$f"
done

if [[ -f /etc/shairport-sync.conf ]]; then
  sudo cp /etc/shairport-sync.conf "/etc/shairport-sync.conf.bak.$(date +%Y%m%d%H%M%S)"
fi

sudo systemctl daemon-reload
sudo systemctl reset-failed || true

echo "[4/11] Install build dependencies"
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential git autoconf automake libtool \
  libpopt-dev libconfig-dev libasound2-dev \
  avahi-daemon libavahi-client-dev libssl-dev libsoxr-dev \
  libplist-dev libsodium-dev uuid-dev libgcrypt-dev xxd libplist-utils \
  libavutil-dev libavcodec-dev libavformat-dev libswresample-dev ffmpeg
sudo apt-get install -y --no-install-recommends systemd-dev || true

echo "[5/11] Configure ALSA loopback module"
echo "snd-aloop" | sudo tee /etc/modules-load.d/snd-aloop.conf >/dev/null
echo "options snd-aloop id=Loopback index=2 pcm_substreams=8" \
  | sudo tee /etc/modprobe.d/snd-aloop.conf >/dev/null
sudo modprobe snd-aloop || true

echo "[6/11] Build/install NQPTP"
cd "$HOME"
if [[ ! -d nqptp ]]; then git clone https://github.com/mikebrady/nqptp.git; fi
cd nqptp
git pull --ff-only || true
autoreconf -fi
./configure --with-systemd-startup
make -j"$(nproc)"
sudo make install
sudo systemctl enable --now nqptp

echo "[7/11] Build/install Shairport Sync (AirPlay 2)"
cd "$HOME"
if [[ ! -d shairport-sync ]]; then git clone https://github.com/mikebrady/shairport-sync.git; fi
cd shairport-sync
git pull --ff-only || true
autoreconf -fi
# AirPlay 2 requires Avahi + OpenSSL.
./configure --sysconfdir=/etc --with-alsa --with-soxr --with-avahi \
  --with-ssl=openssl --with-systemd-startup --with-airplay-2 --with-metadata
make -j"$(nproc)"
sudo make install

echo "[8/11] Write Shairport configuration"
sudo install -m 0644 "${CONFIG_SOURCE}" /etc/shairport-sync.conf

echo "[9/11] Configure ALSA default input mapping"
if [[ -f /etc/asound.conf ]]; then
  sudo cp /etc/asound.conf "/etc/asound.conf.bak.$(date +%Y%m%d%H%M%S)"
fi
sudo install -m 0644 "${ASOUND_SOURCE}" /etc/asound.conf
if [[ -f "${HOME}/.asoundrc" ]]; then
  cp "${HOME}/.asoundrc" "${HOME}/.asoundrc.bak.$(date +%Y%m%d%H%M%S)"
fi
install -m 0644 "${ASOUND_SOURCE}" "${HOME}/.asoundrc"

echo "[10/11] Ensure metadata pipe exists at boot and now"
echo "p /tmp/shairport-sync-metadata 0666 root root -" \
  | sudo tee /etc/tmpfiles.d/shairport-sync-metadata.conf >/dev/null
sudo systemd-tmpfiles --create /etc/tmpfiles.d/shairport-sync-metadata.conf
sudo rm -f /tmp/shairport-sync-metadata
sudo mkfifo /tmp/shairport-sync-metadata
sudo chmod 0666 /tmp/shairport-sync-metadata

echo "[11/11] Enable/start services and verify"
sudo systemctl enable --now avahi-daemon shairport-sync
sudo systemctl restart shairport-sync

echo
echo "Versions:"
nqptp -V || true
shairport-sync -V || true

echo
echo "Verify Shairport build capabilities (AirPlay2):"
SHAIRPORT_VERSION="$(shairport-sync -V 2>/dev/null || true)"
if ! echo "${SHAIRPORT_VERSION}" | grep -q "AirPlay2"; then
  echo "ERROR: Installed shairport-sync does not report AirPlay2 support: ${SHAIRPORT_VERSION}" >&2
  exit 1
fi
echo "OK: ${SHAIRPORT_VERSION}"

echo
echo "Verify FFmpeg AAC decoder supports fltp (required for AirPlay 2 buffered AAC):"
AAC_DECODER_INFO="$(ffmpeg -hide_banner -h decoder=aac 2>/dev/null || true)"
if ! echo "${AAC_DECODER_INFO}" | grep -qi "Supported sample formats:.*fltp"; then
  echo "ERROR: ffmpeg AAC decoder does not report fltp support." >&2
  echo "Install/repair FFmpeg so decoder=aac supports planar float." >&2
  exit 1
fi
echo "OK: AAC decoder reports fltp support."

echo
echo "Service status:"
systemctl --no-pager --full status nqptp shairport-sync | sed -n '1,120p'
echo
echo "ALSA devices:"
aplay -l | sed -n '1,120p'
arecord -l | sed -n '1,120p'
echo
echo "Metadata pipe:"
ls -l /tmp/shairport-sync-metadata || true
