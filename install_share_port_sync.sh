#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
CONFIG_SOURCE="${SCRIPT_DIR}/piWebServer/shairport-sync.conf"

if [[ ! -f "${CONFIG_SOURCE}" ]]; then
  echo "ERROR: Expected config file not found: ${CONFIG_SOURCE}" >&2
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

echo "[0/8] Validate sudo access"
sudo -v

echo "[1/8] Stop old services"
sudo systemctl disable --now shairport-sync >/dev/null 2>&1 || true
sudo systemctl disable --now nqptp >/dev/null 2>&1 || true

echo "[2/8] Remove distro packages (if present)"
sudo apt-get remove -y shairport-sync nqptp || true
sudo apt-get autoremove -y

echo "[3/8] Remove old manual binaries/service files"
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

echo "[4/8] Install build dependencies"
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential git autoconf automake libtool \
  libpopt-dev libconfig-dev libasound2-dev \
  avahi-daemon libavahi-client-dev libssl-dev libsoxr-dev \
  libplist-dev libsodium-dev uuid-dev libgcrypt-dev xxd libplist-utils \
  libavutil-dev libavcodec-dev libavformat-dev
sudo apt-get install -y --no-install-recommends systemd-dev || true

echo "[5/8] Build/install NQPTP"
cd "$HOME"
if [[ ! -d nqptp ]]; then git clone https://github.com/mikebrady/nqptp.git; fi
cd nqptp
git pull --ff-only || true
autoreconf -fi
./configure --with-systemd-startup
make -j"$(nproc)"
sudo make install
sudo systemctl enable --now nqptp

echo "[6/8] Build/install Shairport Sync (AirPlay 2)"
cd "$HOME"
if [[ ! -d shairport-sync ]]; then git clone https://github.com/mikebrady/shairport-sync.git; fi
cd shairport-sync
git pull --ff-only || true
autoreconf -fi
./configure --sysconfdir=/etc --with-alsa --with-soxr --with-avahi \
  --with-ssl=openssl --with-systemd-startup --with-airplay-2
make -j"$(nproc)"
sudo make install

echo "[7/8] Write minimal config"
sudo install -m 0644 "${CONFIG_SOURCE}" /etc/shairport-sync.conf

echo "[8/8] Enable/start services and verify"
sudo systemctl enable --now avahi-daemon shairport-sync
sudo systemctl restart shairport-sync

echo
echo "Versions:"
nqptp -V || true
shairport-sync -V || true

echo
echo "Service status:"
systemctl --no-pager --full status nqptp shairport-sync | sed -n '1,120p'
