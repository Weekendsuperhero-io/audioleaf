# Debug runbook: AirPlay receiver disappears after long pause

## Symptom

On the Raspberry Pi, the **"Nano Viz"** AirPlay receiver disappears from the
iPhone/Mac AirPlay picker after audio is paused for a long time. Even
`podman compose down && podman compose up -d` does not bring it back.

The fact that a full container restart doesn't fix it is the surprising part —
it points at host-side state (snd-aloop kernel module, host avahi conflict on
UDP 5353, source-device caches) or a startup-order problem rather than a
process that simply crashed.

## Why we can't diagnose it yet

Out of the box, `podman logs audioleaf` only shows audioleaf's own output:

- `shairport-sync` defaults to syslog (no syslog daemon inside the container).
- `nqptp` is started in the background but its stderr was inherited silently.
- `avahi-daemon` is started with `--daemonize`, which detaches its log.
- The `diagnostics` block in `piWebServer/shairport-sync.conf` is commented out.

Phase 1 (this runbook) makes the failure observable. Phase 2 (designed after we
have a captured failure) is the actual fix.

## Phase 1 changes already applied

| File | Change |
|---|---|
| `piWebServer/shairport-sync.conf` | `diagnostics` block uncommented, `log_verbosity=2`, file/line + time-since-startup on |
| `containers/entrypoint.sh` | `avahi-daemon` foreground (no `--daemonize`), `shairport-sync -u -vv` (stderr, verbose), all services' stderr redirected, PIDs logged at startup, `cleanup` trap also kills avahi |
| `containers/diag.sh` *(new)* | One-shot state capture: processes, mDNS browse, UDP listeners, `/proc/asound/Loopback/*`, dbus socket |
| `containers/compose.yaml` | Bind-mounts the three files above on top of the published image so we can iterate without rebuild |

To revert: drop the three `:ro,Z` mounts from `containers/compose.yaml` and
re-comment the `diagnostics` block.

## Reproduction & capture protocol

Run on the Pi.

### 1. Start with the diagnostic build and confirm logs

```sh
cd /path/to/audioleaf
podman compose -f containers/compose.yaml down
podman compose -f containers/compose.yaml up -d
podman logs -f audioleaf > /tmp/audioleaf-baseline.log &
```

Within ~5 seconds you should see entrypoint lines plus `shairport-sync`,
`avahi`, and `nqptp` output in the log. If you don't, the bind-mount didn't
land — re-check the volume paths in `containers/compose.yaml`.

### 2. Capture a healthy baseline

```sh
podman exec audioleaf /usr/local/bin/diag.sh > /tmp/healthy-baseline.txt 2>&1
```

This is what "working" looks like. We diff it against the failure capture.

### 3. Reproduce the failure

1. From iPhone/Mac, select **"Nano Viz"** in the AirPlay picker.
2. Play a track for ~30 seconds.
3. **Pause.** Note the timestamp.
4. Walk away. Check the picker every ~15 minutes.

### 4. The instant "Nano Viz" disappears from the picker

Don't fix anything yet. Capture state first:

```sh
# Container-side state
podman exec audioleaf /usr/local/bin/diag.sh > /tmp/failure-diag.txt 2>&1

# Snapshot the streamed log up to the failure
cp /tmp/audioleaf-baseline.log /tmp/failure-podman.log

# Optional: from a SECOND device on the same network, confirm it's not just
# the source device's mDNS cache lying:
avahi-browse -ar -t | grep -i 'nano viz'
```

### 5. Now test whether down/up recovers

```sh
podman compose -f containers/compose.yaml down
podman compose -f containers/compose.yaml up -d
sleep 15
podman exec audioleaf /usr/local/bin/diag.sh > /tmp/failure-after-restart.txt 2>&1
```

If `failure-after-restart.txt` shows shairport/avahi/nqptp running and mDNS
advertising "Nano Viz" but the iPhone/Mac picker still doesn't show it → it's
a source-device cache issue, not a server issue. If it still doesn't show in
`avahi-browse` from inside the container after restart → host-side state is
poisoned (most likely).

## What each output tells us

Diff `failure-diag.txt` against `healthy-baseline.txt`:

- **`shairport-sync` missing from `ps -ef`** → it crashed silently. The
  entrypoint runs it with `&` and has no supervisor; tini reaps it but doesn't
  restart. Phase 2 = process supervisor (s6-overlay or simple restart loop) +
  read the stderr in `failure-podman.log` to find the crash reason.
- **`shairport-sync` alive, but `avahi-browse` from inside the container
  doesn't show "Nano Viz"** → mDNS deregistration. Look at `avahi` lines in
  `failure-podman.log` for "withdrawing" / "Server disappeared" / dbus errors.
  Phase 2 = avahi/dbus startup ordering or shairport mDNS renewal.
- **`avahi-browse` shows it from inside the container, but the second device's
  `avahi-browse` doesn't** → multicast/IGMP membership lost on the host.
  Phase 2 = network/host-side, not container.
- **Both `avahi-browse` outputs show it, but iPhone/Mac picker doesn't** →
  source-device cache. Phase 2 = bump shairport's mDNS TXT record on a
  heartbeat to force re-advertise.
- **`/proc/asound/Loopback/pcm*c/sub*/status` shows `state: XRUN` or
  `closed` on the writer side** → the standby-mode workaround
  (`shairport-sync.conf:42–44`, `disable_standby_mode = "always"`) failed.
  Phase 2 = ALSA loopback config tuning.
- **After `down/up`, shairport's stderr shows ALSA `device busy` or PTP
  bind error** → host-side state (kernel module stuck, host `avahi-daemon`
  on UDP 5353, leftover `nqptp` on UDP 319/320). This is the most likely
  explanation for "compose down/up doesn't fix it." Phase 2 = on the host:
  `sudo systemctl stop avahi-daemon` (if running on host),
  `sudo modprobe -r snd_aloop && sudo modprobe snd_aloop`, then start
  the container.

## Files to send back for Phase 2 design

- `/tmp/healthy-baseline.txt`
- `/tmp/failure-diag.txt`
- `/tmp/failure-podman.log` — most important; this is where shairport's actual
  death/error message lives now that diagnostics are on
- `/tmp/failure-after-restart.txt`
- The output of these on the Pi host (not the container):
  ```sh
  systemctl is-active avahi-daemon dbus
  pgrep -fa nqptp
  pgrep -fa shairport-sync
  lsmod | grep snd_aloop
  ```

## Reverting Phase 1

Once root cause is captured:

1. In `containers/compose.yaml`, remove the three `:ro,Z` bind-mounts under `volumes:`.
2. In `piWebServer/shairport-sync.conf`, re-comment the `diagnostics` block
   (or leave at `log_verbosity = 1` for ongoing visibility).
3. In `containers/entrypoint.sh`, drop `-vv` from `shairport-sync` (keep `-u`
   so logs continue to reach `podman logs`).
4. Rebuild and republish the image so changes go into `:latest` rather than
   relying on host bind-mounts.
