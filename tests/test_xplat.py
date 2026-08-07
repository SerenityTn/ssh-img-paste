import io
import json
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

from xplat.ssh_image_paste import (
    Profile,
    ProfileStore,
    build_upload_plan,
    default_config_root,
    generate_remote_name,
    main,
    upload_file,
)


class CommandLineTests(unittest.TestCase):
    def test_profile_create_writes_cross_platform_profile(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = io.StringIO()
            status = main([
                "--config-dir", tmp,
                "profile", "create", "work",
                "--label", "Work host",
                "--host", "work-ssh",
                "--remote-home", "/home/me",
                "--remote-dir", "img-uploads",
            ], stdout=output)

            profile = ProfileStore(Path(tmp)).load("work")

        self.assertEqual(status, 0)
        self.assertEqual(profile.host, "work-ssh")
        self.assertEqual(output.getvalue(), "Created profile work.\n")

    def test_upload_file_command_runs_scp_and_prints_remote_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = ProfileStore(Path(tmp))
            store.save(Profile("work", "Work host", "work-ssh", "/home/me", "img-uploads"))
            image = Path(tmp) / "image.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\nfixture")
            calls = []
            output = io.StringIO()

            status = main(
                ["--config-dir", tmp, "--profile", "work", "upload-file", str(image)],
                stdout=output,
                runner=lambda args: calls.append(args) or 0,
                name_factory=lambda: "fixed.png",
            )

        self.assertEqual(status, 0)
        self.assertEqual(output.getvalue(), "/home/me/img-uploads/fixed.png\n")
        self.assertEqual(calls[0][0], "scp")

    def test_module_entry_point_runs_profile_create(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = subprocess.run([
                sys.executable,
                "-m", "xplat.ssh_image_paste",
                "--config-dir", tmp,
                "profile", "create", "work",
                "--label", "Work host",
                "--host", "work-ssh",
                "--remote-home", "/home/me",
                "--remote-dir", "img-uploads",
            ], text=True, capture_output=True, check=False)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "Created profile work.\n")


class ConfigLocationTests(unittest.TestCase):
    def test_windows_uses_local_app_data(self):
        root = default_config_root(
            platform_name="windows",
            env={"LOCALAPPDATA": r"C:\\Users\\Ada\\AppData\\Local"},
            home=Path(r"C:\\Users\\Ada"),
        )
        self.assertEqual(root, Path(r"C:\\Users\\Ada\\AppData\\Local") / "SSH Image Paste")

    def test_linux_uses_xdg_config_home(self):
        root = default_config_root(
            platform_name="linux",
            env={"XDG_CONFIG_HOME": "/home/ada/.config-custom"},
            home=Path("/home/ada"),
        )
        self.assertEqual(root, Path("/home/ada/.config-custom/ssh-img-paste"))


class ProfileStoreTests(unittest.TestCase):
    def test_profile_round_trip_is_literal_json(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = ProfileStore(root)
            profile = Profile(
                id="work",
                label="Work host",
                host="work-ssh",
                remote_home="/home/me",
                remote_dir="img-uploads",
            )

            store.save(profile)
            loaded = store.load("work")

            self.assertEqual(loaded, profile)
            raw = json.loads((root / "profiles" / "work.json").read_text())
            self.assertEqual(raw["host"], "work-ssh")
            self.assertNotIn("command", raw)

    def test_profile_id_rejects_path_traversal(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = ProfileStore(Path(tmp))
            profile = Profile(
                id="../escape",
                label="Bad",
                host="host",
                remote_home="/home/me",
                remote_dir="img-uploads",
            )

            with self.assertRaisesRegex(ValueError, "profile id"):
                store.save(profile)

    def test_profile_load_rejects_path_traversal(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = ProfileStore(Path(tmp))
            with self.assertRaisesRegex(ValueError, "profile id"):
                store.load("../escape")


class UploadPlanTests(unittest.TestCase):
    def test_remote_name_is_timestamped_and_collision_resistant(self):
        name = generate_remote_name(
            datetime(2026, 8, 7, 15, 37, 0, tzinfo=timezone.utc),
            token="ab12cd34",
        )
        self.assertEqual(name, "ssh-img-20260807-153700-ab12cd34.png")

    def test_scp_arguments_are_an_array_and_preserve_local_spaces(self):
        profile = Profile(
            id="work",
            label="Work host",
            host="work-ssh",
            remote_home="/home/me",
            remote_dir="img-uploads",
        )
        source = Path("/tmp/Screen Shot.png")

        plan = build_upload_plan(profile, source, "ssh-img-20260807-153700-ab12cd34.png")

        self.assertEqual(
            plan.arguments,
            [
                "scp",
                "-o", "BatchMode=yes",
                "-o", "ConnectTimeout=6",
                str(source),
                "work-ssh:/home/me/img-uploads/ssh-img-20260807-153700-ab12cd34.png",
            ],
        )
        self.assertEqual(plan.remote_path, "/home/me/img-uploads/ssh-img-20260807-153700-ab12cd34.png")

    def test_upload_plan_rejects_option_like_host(self):
        profile = Profile(
            id="work",
            label="Work host",
            host="-oProxyCommand=bad",
            remote_home="/home/me",
            remote_dir="img-uploads",
        )

        with self.assertRaisesRegex(ValueError, "host"):
            build_upload_plan(profile, Path("/tmp/image.png"), "safe.png")

    def test_upload_plan_rejects_remote_directory_traversal(self):
        profile = Profile(
            id="work",
            label="Work host",
            host="work-ssh",
            remote_home="/home/me",
            remote_dir="../escape",
        )

        with self.assertRaisesRegex(ValueError, "remote directory"):
            build_upload_plan(profile, Path("/tmp/image.png"), "safe.png")

    def test_upload_plan_rejects_relative_remote_home(self):
        profile = Profile(
            id="work",
            label="Work host",
            host="work-ssh",
            remote_home="home/me",
            remote_dir="img-uploads",
        )

        with self.assertRaisesRegex(ValueError, "remote home"):
            build_upload_plan(profile, Path("/tmp/image.png"), "safe.png")

    def test_upload_plan_rejects_unsafe_remote_name(self):
        profile = Profile("work", "Work host", "work-ssh", "/home/me", "img-uploads")

        with self.assertRaisesRegex(ValueError, "remote name"):
            build_upload_plan(profile, Path("/tmp/image.png"), "../escape.png")

    def test_upload_file_rejects_missing_source(self):
        profile = Profile("work", "Work host", "work-ssh", "/home/me", "img-uploads")

        with self.assertRaisesRegex(ValueError, "source image"):
            upload_file(profile, Path("/does/not/exist.png"), "safe.png", runner=lambda _: 0)

    def test_upload_file_executes_scp_and_returns_remote_path(self):
        profile = Profile(
            id="work",
            label="Work host",
            host="work-ssh",
            remote_home="/home/me",
            remote_dir="img-uploads",
        )
        calls = []

        def runner(arguments):
            calls.append(arguments)
            return 0

        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "image.png"
            source.write_bytes(b"\x89PNG\r\n\x1a\nfixture")
            remote_path = upload_file(profile, source, "safe.png", runner=runner)

        self.assertEqual(remote_path, "/home/me/img-uploads/safe.png")
        self.assertEqual(calls[0][0], "scp")
        self.assertEqual(calls[0][-2], str(source))


if __name__ == "__main__":
    unittest.main()
