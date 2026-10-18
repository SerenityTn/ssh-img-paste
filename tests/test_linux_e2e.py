"""Ubuntu X11 capture -> CLI profile -> real SCP transport end-to-end test.

This exercises the current cross-platform tracer against an ephemeral unprivileged
OpenSSH server. It is not a GNOME/Wayland, tray, portal, or clipboard acceptance
test; those require the production Linux desktop adapters and an interactive VM.
"""

from __future__ import annotations

import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def require_program(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        raise unittest.SkipTest(f"{name} is required for Linux e2e")
    return path


def unused_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def ssh_config_value(path: Path) -> str:
    """Quote a filesystem path as one OpenSSH configuration token."""
    return '"' + str(path).replace("\\", "\\\\").replace('"', '\\"') + '"'


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=3)


def wait_for_sshd(
    port: int,
    process: subprocess.Popen[bytes],
    log: Path,
    ssh: str,
    client_key: Path,
) -> None:
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"sshd exited before readiness: {log.read_text(errors='replace')}")
        try:
            probe = subprocess.run([
                ssh,
                "-p", str(port),
                "-i", str(client_key),
                "-F", "/dev/null",
                "-o", "BatchMode=yes",
                "-o", "HostName=127.0.0.1",
                "-o", "IdentitiesOnly=yes",
                "-o", "IdentityAgent=none",
                "-o", "StrictHostKeyChecking=no",
                "-o", "UserKnownHostsFile=/dev/null",
                "-o", "GlobalKnownHostsFile=/dev/null",
                "-o", "ConnectTimeout=1",
                "localhost", "true",
            ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=2, check=False)
        except subprocess.TimeoutExpired:
            time.sleep(0.05)
            continue
        if probe.returncode == 0 and process.poll() is None:
            return
        time.sleep(0.05)
    raise RuntimeError(f"ephemeral sshd did not become ready: {log.read_text(errors='replace')}")


def start_sshd(
    sshd: str,
    ssh: str,
    tmp: Path,
    host_key: Path,
    client_key: Path,
    authorized_keys: Path,
) -> tuple[subprocess.Popen[bytes], int]:
    """Start sshd with bounded retries if another process wins a selected port."""
    failures: list[str] = []
    for attempt in range(5):
        port = unused_port()
        log = tmp / f"sshd-{attempt}.log"
        config = tmp / f"sshd-{attempt}.conf"
        config.write_text(
            "\n".join([
                f"Port {port}",
                "ListenAddress 127.0.0.1",
                f"HostKey {ssh_config_value(host_key)}",
                f"PidFile {ssh_config_value(tmp / f'sshd-{attempt}.pid')}",
                f"AuthorizedKeysFile {ssh_config_value(authorized_keys)}",
                "PasswordAuthentication no",
                "KbdInteractiveAuthentication no",
                "PubkeyAuthentication yes",
                "StrictModes no",
                "UsePAM no",
                "Subsystem sftp internal-sftp",
                "LogLevel ERROR",
            ]) + "\n",
            encoding="utf-8",
        )
        log_handle = log.open("wb")
        process = subprocess.Popen(
            [sshd, "-D", "-e", "-f", str(config)],
            stdout=subprocess.DEVNULL,
            stderr=log_handle,
        )
        log_handle.close()
        try:
            wait_for_sshd(port, process, log, ssh, client_key)
            return process, port
        except RuntimeError as error:
            failures.append(str(error))
            stop_process(process)
    raise AssertionError("could not start ephemeral sshd after retries:\n" + "\n".join(failures))


class LinuxX11TransportEndToEndTests(unittest.TestCase):
    def test_x11_capture_uploads_over_real_scp(self) -> None:
        if not sys.platform.startswith("linux"):
            self.skipTest("Linux-only end-to-end test")

        xvfb_run = require_program("xvfb-run")
        scrot = require_program("scrot")
        sshd = require_program("sshd")
        ssh = require_program("ssh")
        ssh_keygen = require_program("ssh-keygen")
        real_scp = require_program("scp")

        with tempfile.TemporaryDirectory(prefix="ssh img paste linux e2e ") as raw_tmp:
            tmp = Path(raw_tmp)
            image = tmp / "capture with spaces.png"
            capture_env = os.environ.copy()
            capture_env["TMPDIR"] = "/tmp"  # Debian xvfb-run does not quote whitespace in TMPDIR.
            capture = subprocess.run(
                [xvfb_run, "-a", "-s", "-screen 0 800x600x24 -nolisten tcp", scrot, str(image)],
                env=capture_env,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(capture.returncode, 0, capture.stderr)
            self.assertEqual(image.read_bytes()[:8], b"\x89PNG\r\n\x1a\n")

            host_key = tmp / "host key"
            client_key = tmp / "client key"
            subprocess.run([ssh_keygen, "-q", "-t", "ed25519", "-N", "", "-f", str(host_key)], check=True)
            subprocess.run([ssh_keygen, "-q", "-t", "ed25519", "-N", "", "-f", str(client_key)], check=True)
            authorized_keys = tmp / "authorized keys"
            authorized_keys.write_bytes((tmp / "client key.pub").read_bytes())
            authorized_keys.chmod(0o600)

            sshd_process, port = start_sshd(
                sshd, ssh, tmp, host_key, client_key, authorized_keys
            )
            try:
                remote_home = Path(tempfile.mkdtemp(prefix="ssh-img-e2e-remote-", dir="/tmp"))
                self.addCleanup(shutil.rmtree, remote_home, True)
                wrapper_dir = tmp / "wrapper bin"
                wrapper_dir.mkdir()
                wrapper = wrapper_dir / "scp"
                wrapper.write_text(
                    "#!/usr/bin/env python3\n"
                    "import os, sys\n"
                    "real_scp = os.environ['SSH_IMG_E2E_REAL_SCP']\n"
                    "options = [\n"
                    "    '-P', os.environ['SSH_IMG_E2E_PORT'],\n"
                    "    '-i', os.environ['SSH_IMG_E2E_CLIENT_KEY'],\n"
                    "    '-F', '/dev/null',\n"
                    "    '-o', 'HostName=127.0.0.1',\n"
                    "    '-o', 'IdentitiesOnly=yes',\n"
                    "    '-o', 'IdentityAgent=none',\n"
                    "    '-o', 'StrictHostKeyChecking=no',\n"
                    "    '-o', 'UserKnownHostsFile=/dev/null',\n"
                    "    '-o', 'GlobalKnownHostsFile=/dev/null',\n"
                    "]\n"
                    "os.execv(real_scp, [real_scp, *options, *sys.argv[1:]])\n",
                    encoding="utf-8",
                )
                wrapper.chmod(0o755)

                config = tmp / "config with spaces"
                (remote_home / "uploads").mkdir(parents=True)
                command_env = os.environ.copy()
                command_env.update({
                    "PATH": f"{wrapper_dir}{os.pathsep}{command_env['PATH']}",
                    "SSH_AUTH_SOCK": "",
                    "SSH_IMG_E2E_REAL_SCP": real_scp,
                    "SSH_IMG_E2E_PORT": str(port),
                    "SSH_IMG_E2E_CLIENT_KEY": str(client_key),
                })

                create = subprocess.run([
                    sys.executable, "-m", "xplat.ssh_image_paste",
                    "--config-dir", str(config),
                    "profile", "create", "ubuntu-e2e",
                    "--label", "Ubuntu E2E",
                    "--host", "localhost",
                    "--remote-home", str(remote_home),
                    "--remote-dir", "uploads",
                ], cwd=ROOT, env=command_env, text=True, capture_output=True, check=False)
                self.assertEqual(create.returncode, 0, create.stderr)

                upload = subprocess.run([
                    sys.executable, "-m", "xplat.ssh_image_paste",
                    "--config-dir", str(config),
                    "--profile", "ubuntu-e2e",
                    "upload-file", str(image),
                ], cwd=ROOT, env=command_env, text=True, capture_output=True, check=False)
                self.assertEqual(upload.returncode, 0, upload.stderr)

                uploaded_path = Path(upload.stdout.strip())
                self.assertTrue(uploaded_path.is_file(), upload.stdout)
                self.assertEqual(uploaded_path.read_bytes(), image.read_bytes())
            finally:
                stop_process(sshd_process)


if __name__ == "__main__":
    unittest.main()
