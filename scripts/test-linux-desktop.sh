#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
binary=${AIU_DESKTOP_BINARY:-"$root/target/debug/aiu-desktop"}
fixture=${AIU_DESKTOP_FIXTURE:-"$root/tests/fixtures/frontend/accounts.json"}
timeout_seconds=${AIU_DESKTOP_TIMEOUT:-60}
[[ -x "$binary" && -f "$fixture" ]] || { echo "desktop binary or fixture missing" >&2; exit 2; }

if [[ ${1:-} != --inner ]]; then
  for host in 0 1; do
    echo "Linux desktop smoke: host=$host"
    AIU_SMOKE_HOST=$host dbus-run-session -- xvfb-run -a -e /dev/stderr -s '-screen 0 1280x900x24' bash "$0" --inner
  done
  echo "Linux desktop smoke passed"
  exit 0
fi

tmp=$(mktemp -d)
app=; notifier=; wm=; failed=1
dump() {
  echo "--- smoke diagnostics ---" >&2
  xdotool search --name '.*' getwindowname %@ 2>/dev/null || true
  for log in "$tmp"/*.log; do [[ -f "$log" ]] && { echo "--- $log ---" >&2; cat "$log" >&2; }; done
}
cleanup() {
  [[ $failed == 1 ]] && dump
  kill "$app" "$notifier" "$wm" 2>/dev/null || true
  wait "$app" "$notifier" "$wm" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT

echo "Starting Openbox (DISPLAY=$DISPLAY)"
openbox >"$tmp/openbox.log" 2>&1 & wm=$!
wm_ready() { xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id #'; }
for _ in $(seq 1 "$timeout_seconds"); do
  wm_ready && break
  kill -0 "$wm" || { echo "Openbox exited before becoming ready" >&2; exit 1; }
  sleep 1
done
wm_ready || { echo "Openbox did not become ready" >&2; exit 1; }

if [[ ${AIU_SMOKE_HOST:-0} == 1 ]]; then
  echo "Starting fake StatusNotifier host"
  /usr/bin/python3 "$root/scripts/fake_status_notifier.py" >"$tmp/notifier.log" 2>&1 & notifier=$!
  host_ready() { gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher --method org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierWatcher IsStatusNotifierHostRegistered 2>/dev/null | grep -q true; }
  for _ in $(seq 1 "$timeout_seconds"); do
    host_ready && break
    kill -0 "$notifier" || { echo "StatusNotifier host exited before becoming ready" >&2; exit 1; }
    sleep 1
  done
  host_ready || { echo "StatusNotifier host did not become ready" >&2; exit 1; }
fi
echo "Starting desktop binary"
"$binary" --fixture "$fixture" >"$tmp/app.log" 2>&1 & app=$!
visible() { [[ -n "$(xdotool search --onlyvisible --name '^AIU' 2>/dev/null || true)" ]]; }
hidden() { ! visible; }
wait_for_visible() { for _ in $(seq 1 "$timeout_seconds"); do visible && return 0; ! kill -0 "$app" 2>/dev/null && return 1; sleep 1; done; return 1; }
wait_for_exit() { for _ in $(seq 1 "$timeout_seconds"); do ! kill -0 "$app" 2>/dev/null && return 0; sleep 1; done; return 1; }
close_panel() {
  local window
  window=$(xdotool search --onlyvisible --name '^AIU' | head -1)
  printf -v window '0x%x' "$window"
  wmctrl -ic "$window"
}
wait_for_hidden() {
  for _ in $(seq 1 "$timeout_seconds"); do
    kill -0 "$app" || return 1
    hidden && return 0
    sleep 1
  done
  return 1
}
wait_for_visible || { echo "AIU window did not become visible" >&2; exit 1; }

if [[ ${AIU_SMOKE_HOST:-0} == 0 ]]; then
  echo "Closing without a StatusNotifier host"
  close_panel
  wait_for_exit || { echo "no-host close did not exit" >&2; exit 1; }
  wait "$app"
  failed=0
  exit 0
fi

for _ in $(seq 1 "$timeout_seconds"); do
  grep -q '^registered$' "$tmp/notifier.log" 2>/dev/null && break
  kill -0 "$app" "$notifier" || { echo "desktop or host exited before tray registration" >&2; exit 1; }
  sleep 1
done
grep -q '^registered$' "$tmp/notifier.log" || { echo "tray item was not registered" >&2; exit 1; }
gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher --method org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierWatcher IsStatusNotifierHostRegistered | grep -q true
echo "Closing with a StatusNotifier host"
close_panel
wait_for_hidden || { echo "host close did not hide a running panel" >&2; exit 1; }
gdbus call --session --dest org.aiu.Smoke --object-path /org/aiu/Smoke --method org.aiu.Smoke.Invoke show >/dev/null
wait_for_visible || { echo "Show did not restore panel" >&2; exit 1; }
close_panel
wait_for_hidden || { echo "panel did not hide before Refresh" >&2; exit 1; }
gdbus call --session --dest org.aiu.Smoke --object-path /org/aiu/Smoke --method org.aiu.Smoke.Invoke refresh >/dev/null
sleep 1; hidden || { echo "Refresh unexpectedly showed panel" >&2; exit 1; }; kill -0 "$app"
gdbus call --session --dest org.aiu.Smoke --object-path /org/aiu/Smoke --method org.aiu.Smoke.Invoke quit >/dev/null
wait_for_exit || { echo "Quit did not exit" >&2; exit 1; }
wait "$app"
failed=0
