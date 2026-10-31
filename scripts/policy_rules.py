"""Pure policy predicates shared by repository checks and regression tests."""

from __future__ import annotations

import re

SUPPRESSIONS = [
    re.compile(r"#\s*!?\s*\[[^\]]*\ballow\s*\(", re.IGNORECASE | re.DOTALL),
    re.compile(r"#\s*!?\s*\[[^\]]*\bexpect\s*\(", re.IGNORECASE | re.DOTALL),
    re.compile(r"#\s*\[\s*rustfmt::skip\s*\]", re.IGNORECASE),
    re.compile(r"#\s*" + "no" + r"qa\b", re.IGNORECASE),
    re.compile(r"#\s*type\s*:\s*ignore\b", re.IGNORECASE),
    re.compile(r"#\s*pragma\s*:\s*no\s+cover\b", re.IGNORECASE),
    re.compile(r"#\s*shellcheck\s+disable\s*=", re.IGNORECASE),
    re.compile(r"#\s*pylint\s*:\s*disable\b", re.IGNORECASE),
    re.compile(r"#\s*no" + "sec\b", re.IGNORECASE),
    re.compile(r"gitleaks\s*:\s*" + "allow\b", re.IGNORECASE),
    re.compile(r"//\s*eslint-disable\b", re.IGNORECASE),
    re.compile(r"<!--\s*(?:prettier|markdownlint)-(?:ignore|disable)\b", re.IGNORECASE),
    re.compile(r"/\*\s*stylelint-disable\b", re.IGNORECASE),
    re.compile(r"#\s*fmt\s*:\s*skip\b", re.IGNORECASE),
    re.compile(r"#\s*mypy\s*:\s*ignore-errors\b", re.IGNORECASE),
]
BYPASS_PATTERNS = [
    re.compile(r"--no-" + r"verify\b"),
    re.compile(r"\bSKIP\s*=", re.IGNORECASE),
    re.compile(r"PRE_COMMIT_" + r"ALLOW_NO_CONFIG", re.IGNORECASE),
]
ACTION_PIN = re.compile(r"^\s*uses:\s*([^\s#]+)@([^\s#]+)", re.MULTILINE)
SHA40 = re.compile(r"[0-9a-f]{40}\Z")


def suppression_lines(text: str) -> list[int]:
    lines = {
        text.count("\n", 0, match.start()) + 1
        for pattern in SUPPRESSIONS
        for match in pattern.finditer(text)
    }
    return sorted(lines)


def has_bypass(text: str) -> bool:
    return any(pattern.search(text) for pattern in BYPASS_PATTERNS)


def unpinned_actions(text: str) -> list[str]:
    return [
        action
        for action, revision in ACTION_PIN.findall(text)
        if not action.startswith(("./", "docker://")) and not SHA40.fullmatch(revision)
    ]
