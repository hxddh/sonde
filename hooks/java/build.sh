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
printf 'Premain-Class: dev.sonde.hook.Agent\n' > "$out/manifest"
dest=../../crates/proxy/src/hooks
mkdir -p "$dest"
# the same bytes from the same sources: no timestamps in the jars
find "$out/agent" "$out/caller" -exec touch -d @0 {} +
jar --create --file "$dest/sonde-agent.jar" --manifest "$out/manifest" --date=1980-01-01T00:00:02Z -C "$out/agent" .
jar --create --file "$dest/sonde-caller.jar" --no-manifest --date=1980-01-01T00:00:02Z -C "$out/caller" .
ls -l "$dest"
