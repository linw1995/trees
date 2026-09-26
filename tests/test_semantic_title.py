import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/check_semantic_title.py"


class SemanticTitleTests(unittest.TestCase):
    def check_title(self, title, expected):
        with tempfile.TemporaryDirectory() as directory:
            message = Path(directory) / "COMMIT_EDITMSG"
            message.write_text(title + "\n\nAn unrestricted body.\n", encoding="utf-8")
            for args in ([str(message)], ["--pr-title"]):
                with self.subTest(title=title, args=args):
                    result = subprocess.run(
                        [sys.executable, str(SCRIPT), *args],
                        env={**os.environ, "PR_TITLE": title},
                        capture_output=True,
                        text=True,
                    )
                    self.assertEqual(result.returncode, expected, result.stderr)
                    if expected:
                        self.assertIn("Expected Conventional Commits format", result.stderr)

    def test_valid_headers(self):
        for title in (
            "feat(cli): add workspace status",
            "fix: handle missing config",
            "feat!: change defaults",
            "refactor(workspace)!: update session lifecycle",
            "ci(build/release-v2.0): update workflow",
        ):
            self.check_title(title, 0)

    def test_invalid_headers(self):
        for title in (
            "",
            "add workspace status",
            "Feat(cli): add workspace status",
            "feat(): add workspace status",
            "feat(-cli): add workspace status",
            "feat(cli):add workspace status",
            "feat(cli): ",
            "feat(cli):  add workspace status",
            "feat(cli): add workspace status ",
        ):
            self.check_title(title, 1)

    def test_pr_title_must_be_one_line(self):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--pr-title"],
            env={**os.environ, "PR_TITLE": "feat: add workspace status\nextra"},
            capture_output=True,
        )
        self.assertEqual(result.returncode, 1)

    def test_requires_exactly_one_source(self):
        for args in ([], ["--pr-title", "COMMIT_EDITMSG"]):
            with self.subTest(args=args):
                result = subprocess.run(
                    [sys.executable, str(SCRIPT), *args], capture_output=True, text=True
                )
                self.assertEqual(result.returncode, 2)
                self.assertIn("provide exactly one", result.stderr)

    def test_missing_pr_title_is_invalid(self):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--pr-title"],
            env={key: value for key, value in os.environ.items() if key != "PR_TITLE"},
            capture_output=True,
        )
        self.assertEqual(result.returncode, 1)


if __name__ == "__main__":
    unittest.main()
