#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
binary=${AIU_DESKTOP_BINARY:-"$root/target/debug/aiu-desktop"}
fixture=${AIU_DESKTOP_FIXTURE:-"$root/tests/fixtures/frontend/accounts.json"}
timeout_seconds=${AIU_DESKTOP_TIMEOUT:-20}
[[ -x "$binary" ]] || { echo "missing desktop binary: $binary" >&2; exit 2; }
[[ -f "$fixture" ]] || { echo "missing fixture: $fixture" >&2; exit 2; }

run_case() {
  local with_host=$1
  dbus-run-session -- bash -s -- "$with_host" "$root" "$binary" "$fixture" "$timeout_seconds" <<'SCRIPT'
set -euo pipefail
host=$1; root=$2; binary=$3; fixture=$4; timeout_seconds=$5
export DISPLAY=:99
Xvfb "$DISPLAY" -screen 0 1280x900x24 >/tmp/aiu-xvfb.log 2>&1 & xvfb=$!
openbox >/tmp/aiu-openbox.log 2>&1 & wm=$!
notifier=
app=
cleanup() {
  for pid in "$app" "$notifier" "$wm" "$xvfb"; do
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
  done
  wait "$app" "$notifier" "$wm" "$xvfb" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1
if [[ "$host" == 1 ]]; then
  python3 "$root/scripts/fake_status_notifier.py" >/tmp/aiu-notifier.log 2>&1 & notifier=$!
  sleep 1
fi
"$binary" --fixture "$fixture" >/tmp/aiu-desktop.out 2>/tmp/aiu-desktop.err & app=$!
for _ in $(seq 1 "$timeout_seconds"); do
  xdotool search --onlyvisible --name '^AIU' >/dev/null 2>&1 && break
  sleep 1
done
win=$(xdotool search --onlyvisible --name '^AIU' | head -1)
[[ -n "$win" ]] || { cat /tmp/aiu-desktop.err; exit 1; }
if [[ "$host" == 1 ]]; then
  for _ in $(seq 1 "$timeout_seconds"); do grep -q show-aiu-dispatched /tmp/aiu-notifier.log && break; sleep 1; done
  grep -q show-aiu-dispatched /tmp/aiu-notifier.log
  kill -0 "$app"
  for _ in $(seq 1 "$timeout_seconds"); do grep -q refresh-usage-dispatched /tmp/aiu-notifier.log && break; sleep 1; done
  grep -q refresh-usage-dispatched /tmp/aiu-notifier.log
  kill -0 "$app"
  for _ in $(seq 1 "$timeout_seconds"); do grep -q quit-aiu-dispatched /tmp/aiu-notifier.log && break; sleep 1; done
  grep -q quit-aiu-dispatched /tmp/aiu-notifier.log
  for _ in $(seq 1 "$timeout_seconds"); do kill -0 "$app" 2>/dev/null || exit 0; sleep 1; done
  echo 'desktop did not exit after tray quit' >&2; exit 1
else
  xdotool windowclose "$win"
  for _ in $(seq 1 "$timeout_seconds"); do kill -0 "$app" 2>/dev/null || exit 0; sleep 1; done
  echo 'desktop did not exit after window close without a host' >&2; exit 1
fi
SCRIPT
}
run_case 0
run_case 1
echo "Linux desktop smoke passed"
