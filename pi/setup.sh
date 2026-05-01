#!/usr/bin/env bash
# audioleaf — Raspberry Pi container setup (podman only).
#
# Usage:
#   sudo ./pi/setup.sh                      # full install + deploy
#   curl -fsSL https://raw.githubusercontent.com/Weekendsuperhero-io/audioleaf/main/pi/setup.sh | sudo bash
#
# Flags:
#   --no-systemd          skip writing/enabling audioleaf.service
#   --no-deploy           host prep only (don't pull/start the container)
#   --force-compose       overwrite /etc/audioleaf/compose.yaml if it exists
#   --config-dir=DIR      override /etc/audioleaf

set -euo pipefail

# ---------- defaults ----------
ENABLE_SYSTEMD=1
DEPLOY=1
FORCE_COMPOSE=0
CONFIG_DIR="/etc/audioleaf"
COMPOSE_URL="https://raw.githubusercontent.com/Weekendsuperhero-io/audioleaf/main/containers/compose.yaml"
QUADLET_URL="https://raw.githubusercontent.com/Weekendsuperhero-io/audioleaf/main/containers/audioleaf.container"

TARGET_USER="${SUDO_USER:-${USER:-}}"
SCRIPT_DIR=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
    SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
fi

# ---------- arg parsing ----------
for arg in "$@"; do
    case "$arg" in
        --no-systemd)         ENABLE_SYSTEMD=0 ;;
        --no-deploy)          DEPLOY=0 ;;
        --force-compose)      FORCE_COMPOSE=1 ;;
        --config-dir=*)       CONFIG_DIR="${arg#*=}" ;;
        -h|--help)
            sed -n '2,12p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            echo "ERROR: unknown flag: $arg" >&2
            exit 2
            ;;
    esac
done

# ---------- helpers ----------
banner() { printf '\n[%s/9] %s\n' "$1" "$2"; }
log()    { printf '  %s\n' "$*"; }
warn()   { printf '  WARN: %s\n' "$*" >&2; }
die()    { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

# ---------- preflight ----------
[[ "$(uname -s)" == "Linux" ]] || die "This script targets Linux (Raspberry Pi OS / Debian)."
command -v apt-get >/dev/null   || die "apt-get not found. This script targets Debian-based hosts."
command -v systemctl >/dev/null || die "systemctl not found. systemd is required."

if [[ $EUID -ne 0 ]]; then
    if command -v sudo >/dev/null; then
        log "Re-executing under sudo..."
        exec sudo -E bash "$0" "$@"
    fi
    die "Must run as root (or via sudo)."
fi

if [[ -z "$TARGET_USER" || "$TARGET_USER" == "root" ]]; then
    warn "No non-root \$SUDO_USER detected; group memberships will be skipped."
    TARGET_USER=""
fi

# ---------- 1. install OS packages ----------
banner 1 "Install OS packages"
NEED_INSTALL=()
for pkg in podman podman-compose alsa-utils ca-certificates curl; do
    if ! dpkg -s "$pkg" >/dev/null 2>&1; then
        NEED_INSTALL+=("$pkg")
    fi
done
if (( ${#NEED_INSTALL[@]} )); then
    log "Installing: ${NEED_INSTALL[*]}"
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${NEED_INSTALL[@]}"
else
    log "All packages already installed."
fi

# Decide which compose invocation to use. Prefer the v5+ plugin (`podman compose`)
# and fall back to the legacy standalone `podman-compose` binary.
if podman compose version >/dev/null 2>&1; then
    COMPOSE_CMD=(podman compose)
elif command -v podman-compose >/dev/null 2>&1; then
    COMPOSE_CMD=(podman-compose)
else
    die "Neither 'podman compose' nor 'podman-compose' is available after install."
fi
log "Using compose: ${COMPOSE_CMD[*]}"

# ---------- 2. group memberships ----------
banner 2 "Group memberships"
if [[ -n "$TARGET_USER" ]]; then
    current_groups="$(id -nG "$TARGET_USER" 2>/dev/null || echo "")"
    # audio  — ALSA device access for native runs
    # render — some Pi GPU/audio paths use it
    # systemd-journal — read system journal without sudo (`journalctl -fu audioleaf`)
    for grp in audio render systemd-journal; do
        if getent group "$grp" >/dev/null; then
            if [[ " $current_groups " == *" $grp "* ]]; then
                log "$TARGET_USER already in '$grp'."
            else
                usermod -aG "$grp" "$TARGET_USER"
                log "Added $TARGET_USER to '$grp'."
            fi
        else
            log "Group '$grp' not present on this host; skipping."
        fi
    done
    log "Note: new group memberships take effect after next login."
else
    log "Skipped (no target user)."
fi

# ---------- 3. snd-aloop kernel module ----------
banner 3 "Configure snd-aloop kernel module"
mkdir -p /etc/modules-load.d /etc/modprobe.d
echo "snd-aloop" > /etc/modules-load.d/snd-aloop.conf
cat > /etc/modprobe.d/snd-aloop.conf <<'EOF'
options snd-aloop id=Loopback index=2 pcm_substreams=8
EOF

needs_reload=0
if lsmod | grep -q '^snd_aloop'; then
    current_id=""
    if [[ -r /sys/module/snd_aloop/parameters/id ]]; then
        current_id="$(tr -d '\0\n ' </sys/module/snd_aloop/parameters/id)"
    fi
    if [[ "$current_id" != "Loopback" ]]; then
        log "snd-aloop loaded with id='$current_id'; reloading with 'Loopback'."
        needs_reload=1
    else
        log "snd-aloop already loaded with id=Loopback."
    fi
else
    log "snd-aloop not loaded; loading now."
    needs_reload=1
fi

if (( needs_reload )); then
    modprobe -r snd-aloop 2>/dev/null || true
    if ! modprobe snd-aloop; then
        warn "modprobe snd-aloop failed. The kernel module package may be missing."
    fi
fi

if grep -q Loopback /proc/asound/cards 2>/dev/null; then
    log "Verified: 'Loopback' present in /proc/asound/cards."
else
    warn "'Loopback' card not present in /proc/asound/cards. Audio capture will fail until this is resolved."
fi

# ---------- 4. stage compose + config ----------
banner 4 "Stage compose file + config dir"
mkdir -p "$CONFIG_DIR/config"
chmod 0755 "$CONFIG_DIR" "$CONFIG_DIR/config"

compose_dest="$CONFIG_DIR/compose.yaml"
local_compose=""
if [[ -n "$SCRIPT_DIR" && -f "$SCRIPT_DIR/../containers/compose.yaml" ]]; then
    local_compose="$SCRIPT_DIR/../containers/compose.yaml"
fi

if [[ -f "$compose_dest" && $FORCE_COMPOSE -eq 0 ]]; then
    log "$compose_dest exists; preserving (use --force-compose to overwrite)."
elif [[ -n "$local_compose" ]]; then
    cp "$local_compose" "$compose_dest"
    log "Copied compose.yaml from local clone."
else
    if curl -fsSL "$COMPOSE_URL" -o "$compose_dest"; then
        log "Fetched compose.yaml from $COMPOSE_URL."
    else
        die "Failed to fetch $COMPOSE_URL"
    fi
fi

# ---------- 5. pull image ----------
banner 5 "Pull container image"
if (( DEPLOY )); then
    ( cd "$CONFIG_DIR" && "${COMPOSE_CMD[@]}" pull )
else
    log "Skipped (--no-deploy)."
fi

# ---------- 6. install Quadlet ----------
banner 6 "Install Podman Quadlet"
if (( DEPLOY )) && (( ENABLE_SYSTEMD )); then
    # Quadlets require podman >= 4.4 (the systemd generator that translates
    # .container files into .service units).
    podman_version="$(podman version --format '{{.Client.Version}}' 2>/dev/null || echo 0)"
    podman_major="${podman_version%%.*}"
    podman_minor_full="${podman_version#*.}"
    podman_minor="${podman_minor_full%%.*}"
    if [[ ! "$podman_major" =~ ^[0-9]+$ ]] || [[ ! "$podman_minor" =~ ^[0-9]+$ ]] \
       || (( podman_major < 4 )) \
       || (( podman_major == 4 && podman_minor < 4 )); then
        die "Podman $podman_version is too old for Quadlets (need >= 4.4). Re-run with --no-systemd to use compose, or upgrade podman."
    fi
    log "Podman $podman_version supports Quadlets."

    # Source for the Quadlet template: prefer local clone, else fetch from main.
    local_quadlet=""
    if [[ -n "$SCRIPT_DIR" && -f "$SCRIPT_DIR/../containers/audioleaf.container" ]]; then
        local_quadlet="$SCRIPT_DIR/../containers/audioleaf.container"
    fi

    quadlet_dir="/etc/containers/systemd"
    quadlet_dest="$quadlet_dir/audioleaf.container"
    mkdir -p "$quadlet_dir"

    if [[ -n "$local_quadlet" ]]; then
        quadlet_src="$local_quadlet"
        log "Installing Quadlet from local clone."
    else
        quadlet_src="$(mktemp)"
        if ! curl -fsSL "$QUADLET_URL" -o "$quadlet_src"; then
            rm -f "$quadlet_src"
            die "Failed to fetch $QUADLET_URL"
        fi
        log "Fetched Quadlet from $QUADLET_URL."
    fi

    # Substitute the volume mount to honor --config-dir. The default in the
    # template is /etc/audioleaf/config — only rewrite when CONFIG_DIR differs.
    if [[ "$CONFIG_DIR" != "/etc/audioleaf" ]]; then
        sed "s|^Volume=/etc/audioleaf/config:|Volume=${CONFIG_DIR}/config:|" \
            "$quadlet_src" > "$quadlet_dest"
        log "Rewrote Volume= line for --config-dir=$CONFIG_DIR."
    else
        cp "$quadlet_src" "$quadlet_dest"
    fi
    chmod 0644 "$quadlet_dest"

    # Clean up the temp file if we used one.
    [[ -z "$local_quadlet" ]] && rm -f "$quadlet_src"

    # The Quadlet generator runs at daemon-reload and turns .container files
    # into hidden .service units in /run/systemd/generator/.
    systemctl daemon-reload

    # Even with [Install] WantedBy= in the .container, you must explicitly
    # enable the generated .service to wire it to the boot target.
    systemctl enable audioleaf.service
    systemctl restart audioleaf.service
    log "audioleaf.service enabled and started via Quadlet."
else
    log "Skipped ($([[ $DEPLOY -eq 0 ]] && echo --no-deploy || echo --no-systemd))."
fi

# ---------- 7. polkit rule (no-sudo systemctl) ----------
banner 7 "polkit rule for no-sudo service control"
if (( ENABLE_SYSTEMD )); then
    polkit_rules_dir="/etc/polkit-1/rules.d"
    if [[ -d "$polkit_rules_dir" ]]; then
        cat >"$polkit_rules_dir/50-audioleaf.rules" <<'POLKIT'
// Allow members of the 'audio' group to start/stop/restart/enable/disable
// audioleaf.service without a password prompt or sudo.
// Installed by audioleaf's pi/setup.sh.
polkit.addRule(function(action, subject) {
    if (action.id == "org.freedesktop.systemd1.manage-units" &&
        action.lookup("unit") == "audioleaf.service" &&
        subject.isInGroup("audio")) {
        return polkit.Result.YES;
    }
});
POLKIT
        chmod 0644 "$polkit_rules_dir/50-audioleaf.rules"
        log "Installed $polkit_rules_dir/50-audioleaf.rules (audio-group → manage audioleaf.service)."
    else
        warn "$polkit_rules_dir not present; skipping. Install 'polkitd' for no-sudo systemctl."
    fi
else
    log "Skipped (--no-systemd: no service to manage)."
fi

# ---------- 8. start (when not using systemd) ----------
banner 8 "Start container"
if (( DEPLOY )) && (( ! ENABLE_SYSTEMD )); then
    ( cd "$CONFIG_DIR" && "${COMPOSE_CMD[@]}" up -d )
    log "Container started via compose."
elif (( ! DEPLOY )); then
    log "Skipped (--no-deploy)."
else
    log "Started by Quadlet-generated audioleaf.service."
fi

# ---------- 9. final report ----------
banner 9 "Done"
host_ip="$(hostname -I 2>/dev/null | awk '{print $1}')"
host_ip="${host_ip:-<pi-ip>}"

cat <<EOF

Audioleaf is set up.

  Web UI:        http://${host_ip}:8787
  Config dir:    $CONFIG_DIR/config
  Compose file:  $compose_dest

Useful commands (no sudo needed once you've logged out and back in):
  journalctl -fu audioleaf                   # live logs
  systemctl status audioleaf                 # service state
  systemctl restart audioleaf                # restart
  sudo podman compose -f $CONFIG_DIR/compose.yaml pull   # update image (still needs sudo for podman)

To enable verbose shairport metadata logging:
  edit $CONFIG_DIR/audioleaf.container (or /etc/containers/systemd/audioleaf.container)
  uncomment the AUDIOLEAF_LOG_METADATA line, then
  sudo systemctl daemon-reload && systemctl restart audioleaf
  journalctl -fu audioleaf | grep META

If you were just added to new groups, log out and log back in for them to apply.
EOF
