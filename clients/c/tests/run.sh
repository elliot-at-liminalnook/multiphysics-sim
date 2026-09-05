#!/bin/sh
# Build the example and check its exact output for a scripted conversation.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
build=$(mktemp -d)
trap 'rm -rf "$build"' EXIT

cc -std=c99 -Wall -Wextra -Werror -I"$here/.." -o "$build/p_controller" "$here/../examples/p_controller.c"

printf '%s\n' \
  '{"type":"hello","element":"controller","period":0.001,"sensors":[{"name":"angle","unit":"rad"},{"name":"speed","unit":"rad/s"}],"actuators":[{"name":"voltage","unit":"V"},{"name":"brake","unit":"1"}]}' \
  '{"type":"sample","seq":0,"t":0.0,"sensors":[0.5,-0.25]}' \
  '{"type":"sample","seq":1,"t":0.001,"sensors":[0.1,1e-3]}' \
  '{"type":"close"}' \
  | "$build/p_controller" 2>"$build/stderr" >"$build/stdout"

cat >"$build/expected" <<'EXPECTED'
{"type":"ready"}
{"type":"act","seq":0,"actuators":[-1,0.5]}
{"type":"act","seq":1,"actuators":[-0.20000000000000001,-0.002]}
EXPECTED
diff "$build/expected" "$build/stdout"

# Error paths: bad seq and an unknown frame type exit 1 with a message on stderr.
for bad in '{"type":"sample","seq":7,"t":0,"sensors":[0,0]}' '{"type":"pause"}'; do
  if printf '%s\n' '{"type":"hello","element":"c","period":1,"sensors":[{"name":"a","unit":""},{"name":"b","unit":""}],"actuators":[]}' "$bad" | "$build/p_controller" >/dev/null 2>"$build/stderr"; then
    echo "expected failure on $bad"; exit 1
  fi
  grep -q 'p_controller: ' "$build/stderr"
done
echo "ok"
