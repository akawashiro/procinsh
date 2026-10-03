#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."
app_binary="$PWD/target/debug/procinsh"

# sudoers permits setcap only on the application path. Temporarily place the
# unit-test executable there and restore the application even if tests fail.
mkdir -p "$PWD/target"
test_dir=$(mktemp -d "$PWD/target/dev-test.XXXXXX")
restore_binary() {
  if [[ -e "$test_dir/original" ]]; then
    mv -f "$test_dir/original" "$app_binary"
  elif [[ -e "$test_dir/installed" ]]; then
    rm -f "$app_binary"
  fi
  rm -rf "$test_dir"
}
trap restore_binary EXIT
cargo test --locked --bin procinsh --no-run --message-format=json > "$test_dir/build.json"
test_binary=$(python3 - "$test_dir/build.json" <<'PY'
import json
import sys

with open(sys.argv[1]) as output:
    for line in output:
        message = json.loads(line)
        if (message.get("reason") == "compiler-artifact"
                and message.get("profile", {}).get("test")
                and message.get("executable")
                and message["target"]["name"] == "procinsh"):
            print(message["executable"])
            break
    else:
        raise SystemExit("Cargo did not produce the procinsh test executable")
PY
)
if [[ -e "$app_binary" ]]; then
  mv "$app_binary" "$test_dir/original"
fi
touch "$test_dir/installed"
cp "$test_binary" "$app_binary"
./scripts/dev_run.sh "$@"
