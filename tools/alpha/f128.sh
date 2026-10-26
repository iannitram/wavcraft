#!/bin/bash
# f128 alpha run: build the release app and SoundCraft's VST3 test fixture (a cdylib; read from the
# soundcraft checkout, built into a scratch target dir), install it as a .vst3 bundle in a scratch
# folder, and drive VST3 scan / list / RACK set through the control channel.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p wavcraft
SC="${SOUNDCRAFT_DIR:-$HOME/dev/craft/soundcraft}"
SCRATCH="$(mktemp -d "$HOME/dev/craft/_wt/scratch/f128fx.XXXX")"
FIXTURE=""
if cargo build --quiet --manifest-path "$SC/crates/vst3-host/tests/fixture/Cargo.toml" --target-dir "$SCRATCH/target"; then
  case "$(uname)" in
    Darwin) LIB=libvst3_fixture.dylib; SUB=MacOS; BIN=Fixture ;;
    *) LIB=libvst3_fixture.so; SUB=x86_64-linux; BIN=Fixture.so ;;
  esac
  mkdir -p "$SCRATCH/plugins/vendor/Fixture.vst3/Contents/$SUB"
  cp "$SCRATCH/target/debug/$LIB" "$SCRATCH/plugins/vendor/Fixture.vst3/Contents/$SUB/$BIN"
  FIXTURE="$SCRATCH/plugins"
else
  echo "fixture build failed: the load test will be skipped"
fi
status=0
python3 tools/alpha/f128.py --exe "$CARGO_TARGET_DIR/release/wavcraft" ${FIXTURE:+--plugins "$FIXTURE"} || status=$?
rm -rf "$SCRATCH"
exit $status
