#!/usr/bin/env bash
# Runs the machine's own git hook of the same name, in a clone that asks for it.
#
# Usage: machine-hook.sh <hook> [hook args...]   (stdin is handed on, for pre-push)
#
# The public clone sets core.hooksPath to lbterm/publish/hooks, which replaces the hooks folder the
# machine's global git config names, so the machine's own hooks would never run there. A clone with
# namemask.mode in its git config needs them: each hook in lbterm/publish/hooks calls this first,
# and the machine's hook runs before the leak check. If that hook can't be found, the commit or push
# is blocked. A clone without namemask.mode (anyone else's) runs the leak check alone.

set -eu

hook="${1:?usage: machine-hook.sh <hook> [args...]}"
shift

[ -n "$(git config --get namemask.mode || true)" ] || exit 0

dir="$(git config --global --includes --get core.hooksPath || true)"
if [ -z "$dir" ]; then
  echo "$hook: namemask.mode is set but the global git config has no core.hooksPath. Blocked." >&2
  exit 1
fi
# The clone's own folder would only call this again
if [ "$dir" = "$(git config --local --get core.hooksPath || true)" ]; then
  echo "$hook: the global core.hooksPath is this clone's own hooks folder, not the machine's. Blocked." >&2
  exit 1
fi
if [ ! -f "$dir/$hook" ]; then
  echo "$hook: namemask.mode is set but the machine's $hook hook is missing from $dir. Blocked." >&2
  exit 1
fi
exec sh "$dir/$hook" "$@"
