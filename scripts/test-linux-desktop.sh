#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
binary=${AIU_DESKTOP_BINARY:-"$root/target/debug/aiu-desktop"}
fixture=${AIU_DESKTOP_FIXTURE:-"$root/tests/fixtures/frontend/accounts.json"}
timeout_seconds=${AIU_DESKTOP_TIMEOUT:-60}
[[ -x "$binary" && -f "$fixture" ]] || { echo "desktop binary or fixture missing" >&2; exit 2; }

wait_for() { local n=$1; shift; for _ in $(seq 1 "$timeout_seconds"); do "$@" && return 0; sleep 1; done; return 1; }

run_case() {
  local host=$1
  dbus-run-session -- bash -c '
    set -euo pipefail
    export DISPLAY=:99
    Xvfb "$DISPLAY" -screen 0 1280x900x24 >/tmp/aiu-xvfb.log 2>&1 & xvfb=$!
    cleanup() { kill "$app" "$notifier" "$wm" "$xvfb" 2>/dev/null || true; wait "$app" "$notifier" "$wm" "$xvfb" 2>/dev/null || true; }
    app=; notifier=; wm=; trap cleanup EXIT
    for _ in $(seq 1 "'"$timeout_seconds"'"); do xdpyinfo >/dev/null 2>&1 && break; sleep 1; done
    openbox >/tmp/aiu-openbox.log 2>&1 & wm=$!
    if [[ "'"$host"'" == 1 ]]; then
      /usr/bin/python3 "'"$root"'/scripts/fake_status_notifier.py" >/tmp/aiu-notifier.log 2>&1 & notifier=$!
      for _ in $(seq 1 "'"$timeout_seconds"'"); do
        gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher --method org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierWatcher IsStatusNotifierHostRegistered 2>/dev/null | grep -q true && break
        sleep 1
      done
      kill -0 "$notifier" || { cat /tmp/aiu-notifier.log; exit 1; }
    fi
    "'"$binary"'" --fixture "'"$fixture"'" >/tmp/aiu-desktop.out 2>/tmp/aiu-desktop.err & app=$!
    visible() { [[ -n "$(xdotool search --onlyvisible --name "^AIU" 2>/dev/null || true)" ]]; }
    hidden() { ! visible; }
    wait_for_visible() { for _ in $(seq 1 "'"$timeout_seconds"'"); do visible && return 0; sleep 1; done; return 1; }
    wait_for_exit() { for _ in $(seq 1 "'"$timeout_seconds"'"); do ! kill -0 "$app" 2>/dev/null && return 0; sleep 1; done; return 1; }
    wait_for_visible || { cat /tmp/aiu-desktop.err; exit 1; }
    win=$(xdotool search --onlyvisible --name "^AIU" | head -1)
    if [[ "'"$host"'" == 0 ]]; then
      xdotool windowactivate --sync "$win" key --clearmodifiers alt+F4
      wait_for_exit || { echo "no-host close did not exit"; exit 1; }
      exit 0
    fi
    for _ in $(seq 1 "'"$timeout_seconds"'"); do grep -q registered /tmp/aiu-notifier.log && break; sleep 1; done
    grep -q registered /tmp/aiu-notifier.log || { cat /tmp/aiu-notifier.log; echo "tray item was not registered"; exit 1; }
    gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher --method org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierWatcher IsStatusNotifierHostRegistered | grep -q true
    xdotool windowactivate --sync "$win" key --clearmodifiers alt+F4
    for _ in $(seq 1 "'"$timeout_seconds"'"); do hidden && break; sleep 1; done
    hidden || { echo "host close did not hide panel"; exit 1; }; kill -0 "$app"
    gdbus call --session --dest org.aiu.Smoke --object-path /org/aiu/Smoke --method org.aiu.Smoke.Invoke show >/dev/null
    wait_for_visible || { echo "Show did not restore panel"; exit 1; }
    win=$(xdotool search --onlyvisible --name "^AIU" | head -1)
    xdotool windowactivate --sync "$win" key --clearmodifiers alt+F4
    for _ in $(seq 1 "'"$timeout_seconds"'"); do hidden && break; sleep 1; done
    hidden || { echo "panel did not hide before Refresh"; exit 1; }
    gdbus call --session --dest org.aiu.Smoke --object-path /org/aiu/Smoke --method org.aiu.Smoke.Invoke refresh >/dev/null
    sleep 1; hidden || { echo "Refresh unexpectedly showed panel"; exit 1; }; kill -0 "$app"
    gdbus call --session --dest org.aiu.Smoke --object-path /org/aiu/Smoke --method org.aiu.Smoke.Invoke quit >/dev/null
    wait_for_exit || { echo "Quit did not exit"; exit 1; }
  '
}

run_case 0
run_case 1
echo "Linux desktop smoke passed"
