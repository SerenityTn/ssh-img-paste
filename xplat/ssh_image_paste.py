"""Cross-platform SSH Image Paste core."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import uuid
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path


def generate_remote_name(now: datetime | None = None, token: str | None = None) -> str:
    timestamp = (now or datetime.now(timezone.utc)).astimezone(timezone.utc)
    suffix = token or uuid.uuid4().hex[:8]
    return f"ssh-img-{timestamp:%Y%m%d-%H%M%S}-{suffix}.png"


def default_config_root(platform_name: str, env: dict[str, str], home: Path) -> Path:
    if platform_name == "windows":
        return Path(env.get("LOCALAPPDATA", str(home / "AppData" / "Local"))) / "SSH Image Paste"
    if platform_name == "linux":
        return Path(env.get("XDG_CONFIG_HOME", str(home / ".config"))) / "ssh-img-paste"
    raise ValueError(f"unsupported platform: {platform_name}")


@dataclass(frozen=True)
class UploadPlan:
    arguments: list[str]
    remote_path: str


@dataclass(frozen=True)
class Profile:
    id: str
    label: str
    host: str
    remote_home: str
    remote_dir: str


class ProfileStore:
    def __init__(self, root: Path):
        self.root = Path(root)
        self.profiles = self.root / "profiles"

    def save(self, profile: Profile) -> None:
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", profile.id):
            raise ValueError("invalid profile id")
        self.profiles.mkdir(parents=True, exist_ok=True)
        destination = self.profiles / f"{profile.id}.json"
        fd, temporary = tempfile.mkstemp(prefix=f".{profile.id}.", dir=self.profiles)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                json.dump(asdict(profile), stream, ensure_ascii=False, indent=2)
                stream.write("\n")
                stream.flush()
                os.fsync(stream.fileno())
            if os.name != "nt":
                os.chmod(temporary, 0o600)
            os.replace(temporary, destination)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)

    def load(self, profile_id: str) -> Profile:
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", profile_id):
            raise ValueError("invalid profile id")
        data = json.loads((self.profiles / f"{profile_id}.json").read_text(encoding="utf-8"))
        return Profile(**data)


def build_upload_plan(profile: Profile, source: Path, remote_name: str) -> UploadPlan:
    if not profile.host or profile.host.startswith("-") or not re.fullmatch(r"[A-Za-z0-9._@:-]+", profile.host):
        raise ValueError("invalid host")
    remote_home_parts = profile.remote_home.split("/")
    if (
        not profile.remote_home.startswith("/")
        or not re.fullmatch(r"/[A-Za-z0-9._/-]+", profile.remote_home)
        or any(part in ("", ".", "..") for part in remote_home_parts[1:])
    ):
        raise ValueError("invalid remote home")
    remote_dir_parts = profile.remote_dir.split("/")
    if (
        not profile.remote_dir
        or profile.remote_dir.startswith(("/", "-"))
        or not re.fullmatch(r"[A-Za-z0-9._/-]+", profile.remote_dir)
        or any(part in ("", ".", "..") for part in remote_dir_parts)
    ):
        raise ValueError("invalid remote directory")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*\.png", remote_name):
        raise ValueError("invalid remote name")
    remote_path = f"{profile.remote_home.rstrip('/')}/{profile.remote_dir.strip('/')}/{remote_name}"
    return UploadPlan(
        arguments=[
            "scp",
            "-o", "BatchMode=yes",
            "-o", "ConnectTimeout=6",
            str(source),
            f"{profile.host}:{remote_path}",
        ],
        remote_path=remote_path,
    )


def upload_file(profile: Profile, source: Path, remote_name: str, runner=None) -> str:
    source = Path(source)
    if not source.is_file():
        raise ValueError("source image does not exist or is not a regular file")
    plan = build_upload_plan(profile, source, remote_name)
    if runner is None:
        runner = lambda arguments: subprocess.run(arguments, check=False).returncode
    status = runner(plan.arguments)
    if status != 0:
        raise RuntimeError(f"scp failed with status {status}")
    return plan.remote_path


def main(arguments: list[str] | None = None, stdout=None, runner=None, name_factory=None) -> int:
    parser = argparse.ArgumentParser(prog="ssh-img-paste")
    parser.add_argument("--config-dir", required=True)
    parser.add_argument("--profile")
    commands = parser.add_subparsers(dest="command", required=True)
    upload = commands.add_parser("upload-file")
    upload.add_argument("source")
    profile_command = commands.add_parser("profile")
    profile_actions = profile_command.add_subparsers(dest="profile_action", required=True)
    create = profile_actions.add_parser("create")
    create.add_argument("id")
    create.add_argument("--label", required=True)
    create.add_argument("--host", required=True)
    create.add_argument("--remote-home", required=True)
    create.add_argument("--remote-dir", required=True)
    parsed = parser.parse_args(arguments)
    output = stdout or sys.stdout
    if parsed.command == "upload-file":
        if not parsed.profile:
            parser.error("--profile is required for upload-file")
        profile = ProfileStore(Path(parsed.config_dir)).load(parsed.profile)
        remote_name = (name_factory or generate_remote_name)()
        remote_path = upload_file(profile, Path(parsed.source), remote_name, runner=runner)
        output.write(remote_path + "\n")
        return 0
    if parsed.command == "profile" and parsed.profile_action == "create":
        ProfileStore(Path(parsed.config_dir)).save(Profile(
            id=parsed.id,
            label=parsed.label,
            host=parsed.host,
            remote_home=parsed.remote_home,
            remote_dir=parsed.remote_dir,
        ))
        output.write(f"Created profile {parsed.id}.\n")
        return 0
    return 64


if __name__ == "__main__":
    raise SystemExit(main())
