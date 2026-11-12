#!/usr/bin/env sh
# Builds sonde's Java caller hook (Java 8 bytecode): sonde-agent.jar (the
# -javaagent) and sonde-caller.jar (the SDK interceptor), into
# crates/proxy/src/hooks. The SDK stand-ins in stubs/ are compiled against,
# never packed. Run after changing src/; the jars are committed.
set -eu
cd "$(dirname "$0")"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
mkdir -p "$out/stubs" "$out/agent" "$out/caller"
javac --release 8 -nowarn -d "$out/stubs" $(find stubs -name '*.java')
javac --release 8 -nowarn -cp "$out/stubs" -d "$out/agent" src/dev/sonde/hook/Agent.java
javac --release 8 -nowarn -cp "$out/stubs" -d "$out/caller" src/dev/sonde/hook/Caller.java
cp -r resources/. "$out/caller/"
mkdir -p "$out/agent/META-INF"
printf 'Manifest-Version: 1.0\r\nPremain-Class: dev.sonde.hook.Agent\r\n\r\n' > "$out/agent/META-INF/MANIFEST.MF"
dest="$(pwd)/../../crates/proxy/src/hooks"
mkdir -p "$dest"
# the same bytes from the same sources on any JDK: zip, not jar (whose manifest
# names the JDK that made it), with no timestamps and a fixed order
pack() {
  rm -f "$2"
  find "$1" -exec touch -d @315532802 {} +
  (cd "$1" && find . -type f | LC_ALL=C sort | TZ=UTC zip -X -D -q "$2" -@)
}
pack "$out/agent" "$dest/sonde-agent.jar"
pack "$out/caller" "$dest/sonde-caller.jar"
ls -l "$dest"
