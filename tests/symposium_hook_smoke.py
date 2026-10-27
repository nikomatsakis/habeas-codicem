#!/usr/bin/env python3
"""Exercise Habeas's crate hook through Symposium's real session-start dispatcher.

Run with `python3 tests/symposium_hook_smoke.py`. Set SYMPOSIUM_BIN to test a
specific cargo-agents binary. The test uses an isolated configuration and
replaces only the Cargo registry install of Habeas with this checkout's CLI.
"""

import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile


def main():
    repo = Path(__file__).resolve().parents[1]
    cargo = shutil.which("cargo")
    symposium = os.environ.get("SYMPOSIUM_BIN") or shutil.which("cargo-agents")
    if cargo is None or symposium is None:
        raise RuntimeError("cargo and cargo-agents must be installed")

    subprocess.run([cargo, "build", "--locked", "--bin", "habeas"], cwd=repo, check=True)
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    if not target_dir.is_absolute():
        target_dir = repo / target_dir
    habeas = target_dir / "debug" / ("habeas.exe" if os.name == "nt" else "habeas")

    with tempfile.TemporaryDirectory(prefix="habeas-symposium-hook-") as temporary:
        root = Path(temporary)
        project = root / "consumer"
        cargo_home = root / "cargo-home"
        symposium_home = root / "symposium-home"
        wrapper_dir = root / "wrapper"
        for directory in (
            project / "md",
            project / "src",
            cargo_home / "bin",
            symposium_home,
            wrapper_dir,
        ):
            directory.mkdir(parents=True)

        (project / "Cargo.toml").write_text(
            '[package]\nname = "habeas-hook-consumer"\nversion = "0.1.0"\n'
            'edition = "2024"\n\n[dev-dependencies]\n'
            f"habeas = {{ path = {json.dumps(str(repo))} }}\n"
        )
        (project / "Cargo.lock").write_bytes((repo / "Cargo.lock").read_bytes())
        (project / "book.toml").write_text('[book]\nsrc = "md"\n')
        (project / "md" / "SUMMARY.md").write_text("# Summary\n")
        (project / "src" / "lib.rs").write_text("")
        (symposium_home / "config.toml").write_text(
            'auto-sync = false\nauto-update = "off"\nhook-scope = "project"\n'
            'agents-syncing = false\n\n[[agent]]\nname = "codex"\n\n'
            "[defaults]\nsymposium-recommendations = false\nuser-plugins = false\n\n"
            '[plugins]\nauto-enable = ["habeas"]\n'
        )

        # Replace only the registry install of Habeas with this checkout's
        # already-built CLI. All other Cargo commands use the real binary.
        wrapper = wrapper_dir / "cargo"
        marker = root / "habeas-install-invoked"
        original_cargo_home = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))
        wrapper.write_text(
            "#!/bin/sh\n"
            'if [ "$1" = "binstall" ]; then exit 1; fi\n'
            'if [ "$1" = "install" ]; then\n'
            '  for argument in "$@"; do\n'
            '    case "$argument" in\n'
            '      habeas|habeas@*)\n'
            f"        cp {shlex.quote(str(habeas))} {shlex.quote(str(cargo_home / 'bin' / habeas.name))}\n"
            f"        touch {shlex.quote(str(marker))}\n"
            "        exit 0\n"
            "        ;;\n"
            "    esac\n"
            "  done\n"
            "fi\n"
            f"export CARGO_HOME={shlex.quote(original_cargo_home)}\n"
            f"exec {shlex.quote(cargo)} \"$@\"\n"
        )
        wrapper.chmod(0o755)

        env = os.environ.copy()
        env.update(
            CARGO_HOME=str(cargo_home),
            SYMPOSIUM_HOME=str(symposium_home),
            PATH=os.pathsep.join((str(wrapper_dir), str(cargo_home / "bin"), env["PATH"])),
        )
        metadata = subprocess.run(
            [str(wrapper), "metadata", "--format-version", "1", "--all-features"],
            cwd=project,
            env=env,
            text=True,
            capture_output=True,
        )
        if metadata.returncode:
            raise RuntimeError(f"Consumer Cargo metadata failed:\n{metadata.stderr}")
        # {test}`requirements.integration.symposium_setup_hook`
        sync = subprocess.run(
            [symposium, "sync"],
            cwd=project,
            env=env,
            text=True,
            capture_output=True,
        )
        if sync.returncode:
            raise RuntimeError(f"Consumer Symposium sync failed:\n{sync.stderr}")
        hook_registration = project / ".codex" / "hooks.json"
        if not hook_registration.is_file() or "session-start" not in hook_registration.read_text():
            raise AssertionError("Symposium did not register its session-start hook")

        last_stderr = ""

        def session_start():
            nonlocal last_stderr
            payload = {
                "hook_event_name": "SessionStart",
                "session_id": "habeas-smoke-test",
                "cwd": str(project),
            }
            result = subprocess.run(
                [symposium, "-v", "hook", "codex", "session-start"],
                cwd=project,
                env=env,
                input=json.dumps(payload),
                text=True,
                capture_output=True,
            )
            last_stderr = result.stderr
            if result.returncode:
                raise RuntimeError(
                    f"Symposium hook exited {result.returncode}:\n{result.stderr}"
                )
            try:
                output = json.loads(result.stdout)
            except json.JSONDecodeError as error:
                raise RuntimeError(f"Invalid Symposium output: {result.stdout}") from error
            return output.get("hookSpecificOutput", {}).get("additionalContext", "")

        incomplete_context = session_start()
        if "Habeas Codicem is not fully configured" not in incomplete_context:
            status = subprocess.run(
                [symposium, "status"],
                cwd=project,
                env=env,
                text=True,
                capture_output=True,
            )
            raise AssertionError(
                "Symposium did not deliver Habeas's setup advisory. "
                f"Agent output: {incomplete_context!r}\n"
                f"Hook diagnostics: {last_stderr}\n"
                f"Plugin status: {status.stdout}\n{status.stderr}"
            )
        if not marker.exists() or not (cargo_home / "bin" / habeas.name).exists():
            raise AssertionError("Symposium did not acquire the Habeas hook command")

        subprocess.run([habeas, "setup", "--path", str(repo)], cwd=project, check=True)
        configured_context = session_start()
        if "Habeas Codicem is not fully configured" in configured_context:
            raise AssertionError("Symposium kept advising setup after it was complete")

    print("Symposium delivered Habeas's setup advisory only when setup was incomplete.")


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, RuntimeError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
