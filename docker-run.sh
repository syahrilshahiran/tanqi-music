#!/usr/bin/env bash
# Build (if needed) and run Tanqi in Docker on an X11/XWayland desktop.
#   ./docker-run.sh           run
#   ./docker-run.sh --build   rebuild the image first
set -euo pipefail
cd "$(dirname "$0")"

[[ "${1:-}" == "--build" || -z "$(docker images -q tanqi-music)" ]] && docker build -t tanqi-music .

# Let the container's uid-1000 user talk to your X server.
xhost +SI:localuser:"$(id -un)" >/dev/null 2>&1 || true

uid="$(id -u)"
args=(
  --rm --name tanqi-music
  --user "$uid"
  -e DISPLAY="${DISPLAY:-:0}"
  -v /tmp/.X11-unix:/tmp/.X11-unix
  -e WEBKIT_DISABLE_COMPOSITING_MODE=1
  -v tanqi-music-data:/data
  --network host            # simplest way to reach a host Ollama on localhost:11434
)
[[ -d /dev/dri ]] && args+=(--device /dev/dri)
# Audio (PulseAudio / PipeWire-pulse)
if [[ -S "${XDG_RUNTIME_DIR:-/run/user/$uid}/pulse/native" ]]; then
  args+=(-v "${XDG_RUNTIME_DIR:-/run/user/$uid}/pulse/native:/run/pulse/native" -e PULSE_SERVER=unix:/run/pulse/native)
fi
# Session bus: desktop notifications + MPRIS media keys
if [[ -S "${XDG_RUNTIME_DIR:-/run/user/$uid}/bus" ]]; then
  args+=(-v "${XDG_RUNTIME_DIR:-/run/user/$uid}/bus:/run/user/$uid/bus" -e DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus")
fi

exec docker run "${args[@]}" tanqi-music
