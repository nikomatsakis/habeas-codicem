#!/usr/bin/env python3
"""Exercise Habeas's CLI, skills, and hook through a real Symposium consumer.

Run with `python3 tests/symposium_hook_smoke.py`. Set SYMPOSIUM_BIN to test a
specific cargo-agents binary. The test uses an isolated configuration and
installs this checkout through Cargo in place of a registry release.
"""

import errno
import json
import os
from pathlib import Path
import pty
import select
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time


def interactive_sync(symposium, project, env):
    child, terminal = pty.fork()
    if child == 0:
        os.chdir(project)
        os.execvpe(symposium, [symposium, "sync"], {**env, "TERM": "xterm"})

    output = bytearray()
    answered = False
    status = None
    deadline = time.monotonic() + 120
    try:
        while status is None and time.monotonic() < deadline:
            ready, _, _ = select.select([terminal], [], [], 1)
            if ready:
                try:
                    chunk = os.read(terminal, 4096)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    chunk = b""
                output.extend(chunk)
                if b"Enable it?" in output and not answered:
                    os.write(terminal, b"\x1b[B\r")
                    answered = True
            completed, result = os.waitpid(child, os.WNOHANG)
            if completed:
                status = result
        if status is None:
            os.kill(child, signal.SIGKILL)
            os.waitpid(child, 0)
            raise RuntimeError("Interactive Symposium sync timed out")
    finally:
        os.close(terminal)

    if not answered or os.waitstatus_to_exitcode(status):
        raise RuntimeError(
            "Interactive Symposium sync did not enable Habeas:\n"
            + output.decode(errors="replace")
        )


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
            "[defaults]\nsymposium-recommendations = false\nuser-plugins = false\n"
        )

        # Replace only the registry source with this checkout. Cargo still
        # performs the installation into the isolated global bin directory.
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
            f"        export CARGO_HOME={shlex.quote(original_cargo_home)}\n"
            f"        {shlex.quote(cargo)} install --path {shlex.quote(str(repo))} "
            f"--root {shlex.quote(str(cargo_home))} --locked --offline --debug || exit $?\n"
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
        current_version = next(
            package["version"]
            for package in json.loads(metadata.stdout)["packages"]
            if package["name"] == "habeas"
        )
        major, minor, patch = map(int, current_version.split("."))
        next_version = f"{major}.{minor}.{patch + 1}"
        # {test}`requirements.integration.symposium_setup_hook`
        interactive_sync(symposium, project, env)
        saved_config = (symposium_home / "config.toml").read_text()
        if "auto-enable" not in saved_config or "habeas" not in saved_config:
            raise AssertionError("Symposium did not record the decision to enable Habeas")
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
        # {test}`requirements.integration.symposium_cli`
        installed_habeas = cargo_home / "bin" / habeas.name
        if not marker.exists() or not installed_habeas.exists():
            raise AssertionError("Symposium did not acquire the Habeas hook command")

        subprocess.run(
            [installed_habeas, "setup", "--path", str(repo)], cwd=project, check=True
        )
        configured_context = session_start()
        if "Habeas Codicem is not fully configured" in configured_context:
            raise AssertionError("Symposium kept advising setup after it was complete")

        # {test}`requirements.integration.symposium`
        skills_dir = project / ".agents" / "skills"
        source_skills = repo / "assets" / "skills"
        skill_names = sorted(
            path.name for path in source_skills.iterdir() if (path / "SKILL.md").is_file()
        )
        if not skill_names:
            raise AssertionError("Habeas did not ship any skills")
        for name in skill_names:
            installed = skills_dir / name / "SKILL.md"
            if not installed.is_file() or installed.read_bytes() != (
                source_skills / name / "SKILL.md"
            ).read_bytes():
                raise AssertionError(f"Symposium did not install the current {name} skill")

        # Symposium 0.5.1 debounces skill comparisons for five seconds and
        # caches workspace dependencies by whole-second Cargo.lock mtime.
        time.sleep(6)
        next_crate = root / "habeas-next"
        (next_crate / "src").mkdir(parents=True)
        (next_crate / "src" / "lib.rs").write_text("")
        shutil.copytree(source_skills, next_crate / "assets" / "skills")
        manifest = (repo / "Cargo.toml").read_text()
        current_version_line = f'version = "{current_version}"'
        if current_version_line not in manifest:
            raise AssertionError("Cargo did not report the current Habeas version")
        (next_crate / "Cargo.toml").write_text(
            manifest.replace(current_version_line, f'version = "{next_version}"', 1)
        )
        changed_skill = next_crate / "assets" / "skills" / skill_names[0] / "SKILL.md"
        changed_skill.write_text(changed_skill.read_text() + "\nUpdated in the next release.\n")

        (project / "Cargo.toml").write_text(
            '[package]\nname = "habeas-hook-consumer"\nversion = "0.1.0"\n'
            'edition = "2024"\n\n[dev-dependencies]\n'
            f"habeas = {{ path = {json.dumps(str(next_crate))} }}\n"
        )
        updated_metadata = subprocess.run(
            [str(wrapper), "metadata", "--format-version", "1", "--all-features"],
            cwd=project,
            env=env,
            text=True,
            capture_output=True,
        )
        if updated_metadata.returncode:
            raise RuntimeError(f"Updated Cargo metadata failed:\n{updated_metadata.stderr}")
        packages = json.loads(updated_metadata.stdout)["packages"]
        if not any(
            package["name"] == "habeas" and package["version"] == next_version
            for package in packages
        ):
            raise AssertionError("Cargo did not select the next Habeas version")

        updated_sync = subprocess.run(
            [symposium, "sync"],
            cwd=project,
            env=env,
            text=True,
            capture_output=True,
        )
        if updated_sync.returncode:
            raise RuntimeError(f"Updated Symposium sync failed:\n{updated_sync.stderr}")
        if (skills_dir / skill_names[0] / "SKILL.md").read_bytes() != changed_skill.read_bytes():
            raise AssertionError("Symposium kept the old skill after the selected version changed")

    print(
        "Symposium installed the CLI and current skills, refreshed changed skills, "
        "and delivered setup advice only when needed."
    )


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, RuntimeError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
