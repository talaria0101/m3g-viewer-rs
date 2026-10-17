#!/bin/bash
# Renders with the Rust viewer and with the Java reference, then diffs every frame
# pixel by pixel (target/debug/imgdiff). Exit status is non-zero if any frame differs.
#
#   tools/compare.sh assets [frames] [viewer options]   walls, snake, obstacles (default 120 frames)
#   tools/compare.sh scenes [frames] [viewer options]   every tests/data/*.m3g   (default 30 frames)
#
# The Java side is the patched original (tools/java-ref); see tools/env.sh for setup.
# Both sides must see the same GL implementation, so run them on the same machine.
set -u
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
cd "$REPO" || exit 2
MODE=${1:-assets}; FRAMES=${2:-}; shift 2 2>/dev/null; OPTS="$*"
cargo build --quiet --bins || exit 2
[ -d "$M3G_JAVA_REF/build/classes" ] || { echo "Java reference not built in $M3G_JAVA_REF"; exit 2; }
status=0

one() { # name path
  local n=$1 f=$2 J="$M3G_FRAMES/java/$1" R="$M3G_FRAMES/rust/$1"
  rm -rf "$J" "$R"; mkdir -p "$J" "$R"
  local jerr rerr
  jerr=$(cd "$M3G_JAVA_REF" && java -Dm3g.dumpDir="$J" -cp "build/classes:lib/*" m3g.viewer.Main --headless --frames "$FRAMES" $OPTS --out "$J/last.png" "$f" 2>&1 | grep -E "ERROR|Exception" | head -2)
  rerr=$(./target/debug/m3g-viewer --frames "$FRAMES" $OPTS --dump-dir "$R" --out "$R/last.png" "$f" 2>&1 | grep -E "ERROR|WARN|panicked" | head -2)
  [ -n "$jerr" ] && echo "$n: java: $jerr"
  [ -n "$rerr" ] && echo "$n: rust: $rerr"
  rm -f "$J/last.png" "$R/last.png"
  local res; res=$(./target/debug/imgdiff "$J" "$R" --quiet 2>&1 | tail -1)
  ./target/debug/imgdiff "$J" "$R" --quiet >/dev/null 2>&1 || status=1
  echo "$n: $res"
}

case "$MODE" in
  assets) FRAMES=${FRAMES:-120}; for a in walls snake obstacles; do one "$a" "$REPO/assets/$a.m3g"; done ;;
  scenes) FRAMES=${FRAMES:-30}
          # M3G_SCENES="name1 name2" limits the run (default: every scene)
          for f in tests/data/*.m3g; do
            n=$(basename "$f" .m3g)
            if [ -n "${M3G_SCENES:-}" ] && ! echo " $M3G_SCENES " | grep -q " $n "; then continue; fi
            one "$n" "$REPO/$f"
          done ;;
  *) echo "usage: $0 assets|scenes [frames] [viewer options]"; exit 2 ;;
esac
exit $status
