#!/usr/bin/env python3
"""Qualify native Library saves and real bubblewrap mounts in isolated state.

Requires a CLI built with the cyberpunk adapter, bwrap and Linux user namespaces.
This is a shell-game fixture, not provider or real-game performance qualification.
Evidence is retained under the new --output directory; nothing uses live saves.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--modde", type=Path, required=True)
    parser.add_argument("--bwrap", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.modde.resolve(strict=True)
    bwrap = args.bwrap.resolve(strict=True)
    root = args.output.absolute()
    root.mkdir()  # Refuse to overwrite another run's evidence.
    home, install, saves, readonly, bin_dir = [root / name for name in
                                             ("home", "game", "saves", "readonly", "bin")]
    for path in (home, install, saves, readonly, bin_dir):
        path.mkdir()
    (bin_dir / "bwrap").symlink_to(bwrap)
    (home / "secret").write_text("not granted")
    (readonly / "asset").write_text("read-only asset")
    (saves / "slot.sav").write_text("original")
    executable = root / "game-script"
    executable.write_text("""#!/bin/sh
set -eu
if [ "$PROBE_SANDBOX" = 1 ]; then
    test ! -e "$HOME/secret"
    test -z "${MODDE_LIVE_SECRET:-}"
    test "$(cat "$READONLY/asset")" = 'read-only asset'
    if touch "$READONLY/forbidden" 2>/dev/null; then exit 31; fi
    test "$(readlink /proc/self/ns/net)" != "$HOST_NET"
fi
(sleep 0.2
if [ -e "$SAVES/slot.sav" ]; then cat "$SAVES/slot.sav" > "$SAVES/before"; fi
printf '%s' "$PROGRESS" > "$SAVES/slot.sav"
exit "${FAIL_CHILD:-0}") &
exit 0
""")
    executable.chmod(0o700)
    # Both the logical entry and its out-of-install symlink target must work.
    (install / "game").symlink_to(executable)
    (install / "save-link").symlink_to(saves)
    config = home / ".config/modde"
    config.mkdir(parents=True)
    (config / "settings.toml").write_text(
        f'[[game_paths]]\ngame_id = "cyberpunk2077"\npath = {json.dumps(str(install))}\n')
    env = {key: value for key, value in os.environ.items()
           if key in ("PATH", "SSL_CERT_FILE", "SSL_CERT_DIR", "NIX_SSL_CERT_FILE")}
    env.update(HOME=str(home), MODDE_DATA_DIR=str(root / "data"),
               XDG_CONFIG_HOME=str(home / ".config"),
               XDG_DATA_HOME=str(home / ".local/share"),
               XDG_CACHE_HOME=str(home / ".cache"), MODDE_DATABASE_BACKEND="sqlite",
               MODDE_LIVE_SECRET="must not enter the sandbox",
               PATH=str(bin_dir) + os.pathsep + env.get("PATH", ""))
    log = root / "commands.log"

    def run(*command, expected=0):
        result = subprocess.run([str(binary), *command], env=env, text=True,
                                capture_output=True, timeout=90, check=False)
        with log.open("a") as file:
            file.write(f"{command!r}\n{result.stdout}{result.stderr}\nexit={result.returncode}\n")
        if result.returncode != expected:
            raise RuntimeError(f"{command!r} exited {result.returncode}, expected {expected}; see {log}")
        return result.stdout

    games = json.loads(run("library", "list", "--json"))["games"]
    game = next(game for game in games if game["game_id"] == "cyberpunk2077")
    for profile in ("original", "other"):
        run("profile", "create", profile, "--game", "cyberpunk2077")
    settings = {"executable": str(install / "game"), "save_directory": str(install / "save-link"),
                "wrappers": [["env"]], "environment": {}, "profile": "original"}
    launch_file = root / "launch.json"

    def configure(profile, sandbox, progress):
        settings.update(profile=profile, sandbox={"enabled": sandbox, "network": False,
                                                  "read_only": [str(readonly)], "writable": []})
        settings["environment"] = {"PROBE_SANDBOX": str(int(sandbox)),
                                   "READONLY": str(readonly), "SAVES": str(saves),
                                   "HOST_NET": os.readlink("/proc/self/ns/net"), "PROGRESS": progress}
        launch_file.write_text(json.dumps(settings))
        run("library", "configure", game["id"], "--file", str(launch_file))

    configure("original", False, "original-progress")
    run("library", "adopt", game["id"], "--profile", "original")
    for profile, sandbox, progress in (("original", False, "original-progress"),
                                       ("other", True, "other-progress"),
                                       ("original", True, "restored-progress")):
        configure(profile, sandbox, progress)
        run("library", "play", game["id"])
        assert (saves / "slot.sav").read_text() == progress
        assert json.loads(run("library", "status"))["session"] is None
    assert (saves / "before").read_text() == "original-progress"
    configure("original", True, "failed-child-progress")
    settings["environment"]["FAIL_CHILD"] = "23"
    launch_file.write_text(json.dumps(settings))
    run("library", "configure", game["id"], "--file", str(launch_file))
    run("library", "play", game["id"], expected=1)
    assert (saves / "slot.sav").read_text() == "failed-child-progress"
    assert json.loads(run("library", "status"))["session"] is None
    assert (home / "secret").read_text() == "not granted"
    assert not (readonly / "forbidden").exists()
    observations = list((config / "sessions").glob("run-*/evidence.json"))
    assert len(observations) == 4
    statuses = []
    for path in observations:
        evidence = json.loads(path.read_text())
        assert evidence["started"] and evidence["completed"]
        statuses.append(evidence["raw_status"])
        assert not path.with_name("request.json").exists()
    assert sorted(statuses) == [0, 0, 0, 23 << 8]
    with binary.open("rb") as file:
        digest = hashlib.file_digest(file, "sha256").hexdigest()
    (root / "receipt.json").write_text(json.dumps({"passed": True, "kind": "native-fixture",
                                                  "successful_runs": 3, "failed_child_runs": 1,
                                                  "modde": str(binary), "binary_sha256": digest,
                                                  "bwrap": str(bwrap)}, indent=2))
    print(f"Native save continuity and real bubblewrap containment passed; evidence: {root}")


if __name__ == "__main__":
    main()
