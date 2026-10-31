from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from scripts.policy_rules import has_bypass, suppression_lines, unpinned_actions

ROOT = Path(__file__).resolve().parents[1]


class RepositoryPolicyTests(unittest.TestCase):
    def test_rust_lint_attributes_are_rejected(self) -> None:
        directive = chr(35) + "[" + "allow" + chr(40) + "clippy::all)]"
        self.assertEqual(suppression_lines(directive), [1])

    def test_nested_multiline_rust_lint_attributes_are_rejected(self) -> None:
        directive = (
            chr(35)
            + "[cfg_attr(feature,\n"
            + "condition, "
            + "allow"
            + chr(40)
            + "clippy::all))]"
        )
        self.assertEqual(suppression_lines(directive), [1])

    def test_python_lint_comments_are_rejected(self) -> None:
        directive = "#" + "no" + "qa: F401"
        self.assertEqual(suppression_lines(directive), [1])

    def test_unrelated_code_is_not_a_suppression(self) -> None:
        self.assertEqual(suppression_lines("#[derive(Debug)]\nfn main() {}"), [])

    def test_precommit_bypass_forms_are_detected(self) -> None:
        self.assertTrue(has_bypass("git commit --no-" + "verify"))
        self.assertTrue(has_bypass("SKIP" + "=clippy pre-commit run"))
        self.assertTrue(has_bypass("PRE_COMMIT_" + "ALLOW_NO_CONFIG=1"))
        self.assertFalse(has_bypass("pre-commit run --all-files"))

    def test_actions_require_full_immutable_revisions(self) -> None:
        pinned = "a" * 40
        self.assertEqual(unpinned_actions(f"uses: actions/checkout@{pinned}\n"), [])
        self.assertEqual(
            unpinned_actions("uses: actions/checkout@v4\n"), ["actions/checkout"]
        )
        self.assertEqual(unpinned_actions("uses: ./local-action\n"), [])


class XmlAssetTests(unittest.TestCase):
    def run_validator(
        self, contents: bytes, name: str
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / name
            path.write_bytes(contents)
            environment = os.environ | {"PYTHONDONTWRITEBYTECODE": "1"}
            return subprocess.run(
                ["python3", "-B", str(ROOT / "scripts" / "validate_xml.py"), str(path)],
                check=False,
                capture_output=True,
                env=environment,
                text=True,
            )

    def test_valid_xml_parses(self) -> None:
        result = self.run_validator(
            b'<Data template="fixture"><Property name="Value" /></Data>', "valid.EXML"
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_malformed_xml_is_rejected(self) -> None:
        result = self.run_validator(b"<Data>", "invalid.MXML")
        self.assertEqual(result.returncode, 1)
        self.assertTrue(result.stderr.startswith("SPSP XML asset validation failed:"))

    def test_entity_declarations_are_rejected(self) -> None:
        contents = b"<" + b"!DOCTYPE" + b" Data><Data/>"
        result = self.run_validator(contents, "entity.xml")
        self.assertEqual(result.returncode, 1)
        self.assertIn("DTD and entity declarations", result.stderr)


if __name__ == "__main__":
    unittest.main()
