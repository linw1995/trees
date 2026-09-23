import subprocess
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/select_previous_release.py"


def select(target, tags):
    result = subprocess.run(
        [sys.executable, str(SCRIPT), target],
        input="\n".join(tags),
        text=True,
        capture_output=True,
        check=True,
    )
    return result.stdout.strip()


class PreviousReleaseTests(unittest.TestCase):
    def test_backport_ignores_newer_published_release(self):
        self.assertEqual(select("v0.2.1", ["v0.3.0", "v0.2.0", "v0.1.0"]), "v0.2.0")

    def test_selects_highest_earlier_semantic_version(self):
        self.assertEqual(
            select("v0.3.0", ["v0.1.0", "v0.2.0", "v0.2.10", "v0.2.9"]),
            "v0.2.10",
        )

    def test_stable_release_skips_prereleases(self):
        self.assertEqual(
            select("v0.2.1", ["v0.2.1-rc.2", "v0.2.1-rc.1", "v0.2.0"]),
            "v0.2.0",
        )

    def test_prerelease_uses_previous_prerelease(self):
        self.assertEqual(
            select("v0.3.0-rc.10", ["v0.3.0-rc.9", "v0.3.0-rc.2", "v0.2.1"]),
            "v0.3.0-rc.9",
        )

    def test_first_prerelease_uses_prior_stable_release(self):
        self.assertEqual(select("v0.3.0-rc.1", ["v0.3.0", "v0.2.1"]), "v0.2.1")

    def test_first_release_has_no_previous_tag(self):
        self.assertEqual(select("v0.1.0", ["v0.2.0", "other-tag"]), "")

    def test_ignores_invalid_semantic_versions(self):
        self.assertEqual(select("v0.2.1", ["v0.2.01", "v0.2.0-rc.01", "v0.2.0"]), "v0.2.0")

    def test_rejects_invalid_target(self):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "v0.2.01"],
            input="v0.2.0\n",
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("Invalid release tag", result.stderr)


if __name__ == "__main__":
    unittest.main()
