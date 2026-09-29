import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class RemoteApplyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.tools = Path(self.temp.name)
        self.marker = self.tools / "python-was-called"
        k3s = self.tools / "k3s"
        k3s.write_text("#!/bin/sh\nexit 0\n")
        k3s.chmod(0o755)
        python = self.tools / "python3"
        python.write_text(f"#!/bin/sh\ntouch '{self.marker}'\nexit 77\n")
        python.chmod(0o755)

    def tearDown(self):
        self.temp.cleanup()

    def run_remote(self, image):
        env = dict(os.environ)
        env["PATH"] = f"{self.tools}:{env['PATH']}"
        return subprocess.run(
            ["bash", str(Path(__file__).with_name("remote.sh")), "/tmp/payload.json", image],
            env=env,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_invalid_image_stops_before_runtime_apply(self):
        result = self.run_remote("not-an-image")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_valid_image_reaches_runtime_apply(self):
        result = self.run_remote("ghcr.io/vantanminh/knotree-registry@sha256:" + "c" * 64)
        self.assertEqual(result.returncode, 77)
        self.assertTrue(self.marker.exists())


if __name__ == "__main__":
    unittest.main()
