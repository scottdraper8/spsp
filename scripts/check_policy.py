"""Enforce SPSP's source, dependency, workflow, and hook invariants."""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

import tomllib
from policy_rules import has_bypass, suppression_lines, unpinned_actions

ROOT = Path(__file__).resolve().parents[1]
SOURCE_SUFFIXES = {
    ".c",
    ".cc",
    ".cpp",
    ".h",
    ".hpp",
    ".json",
    ".ps1",
    ".py",
    ".rs",
    ".sh",
    ".toml",
    ".wgsl",
    ".xml",
    ".yaml",
    ".yml",
}


def fail(messages: list[str]) -> int:
    if messages:
        print("SPSP repository policy failed:", file=sys.stderr)
        for message in messages:
            print(f"- {message}", file=sys.stderr)
        return 1
    print("SPSP repository policy passed")
    return 0


def repository_files() -> list[Path]:
    ignored = {".git", "target", "dist", ".venv", "__pycache__", "node_modules"}
    files = []
    for directory, names, filenames in os.walk(ROOT):
        names[:] = [name for name in names if name not in ignored]
        files.extend(Path(directory) / filename for filename in filenames)
    return files


def source_files() -> list[Path]:
    return [
        path for path in repository_files() if path.suffix.lower() in SOURCE_SUFFIXES
    ]


def check_suppressions(messages: list[str]) -> None:
    for path in source_files():
        text = path.read_text(encoding="utf-8")
        for number in suppression_lines(text):
            messages.append(
                f"{path.relative_to(ROOT)}:{number}: inline suppression is prohibited"
            )


def check_bypasses(messages: list[str]) -> None:
    for path in repository_files():
        relative = path.relative_to(ROOT)
        if relative.parts and relative.parts[0] in {".git", "target", "dist"}:
            continue
        if (
            path.suffix.lower() in SOURCE_SUFFIXES
            or path.name == ".pre-commit-config.yaml"
        ) and has_bypass(path.read_text(encoding="utf-8")):
            messages.append(
                f"{relative}: pre-commit bypass configuration is prohibited"
            )


def check_manifests(messages: list[str]) -> None:
    manifests = sorted(path for path in repository_files() if path.name == "Cargo.toml")
    if not manifests:
        messages.append("Cargo.toml is required")
        return
    sections = ("dependencies", "dev-dependencies", "build-dependencies")

    def inspect(manifest: Path, label: str, dependencies: dict[str, object]) -> None:
        for name, specification in dependencies.items():
            if isinstance(specification, str):
                version = specification
                details: dict[str, object] = {}
            elif isinstance(specification, dict):
                details = specification
                version = details.get("version")
            else:
                messages.append(f"{manifest.relative_to(ROOT)}: invalid {label}.{name}")
                continue
            if details.get("git"):
                messages.append(
                    f"{manifest.relative_to(ROOT)}: {label}.{name} uses a mutable Git dependency"
                )
            if version is not None and not str(version).startswith("="):
                messages.append(
                    f"{manifest.relative_to(ROOT)}: {label}.{name} must use an exact =version pin"
                )

    for manifest in manifests:
        try:
            with manifest.open("rb") as stream:
                document = tomllib.load(stream)
        except (OSError, tomllib.TOMLDecodeError) as error:
            messages.append(
                f"{manifest.relative_to(ROOT)}: cannot parse Cargo manifest: {error}"
            )
            continue
        for section in sections:
            inspect(manifest, section, document.get(section, {}))
            inspect(
                manifest,
                f"workspace.{section}",
                document.get("workspace", {}).get(section, {}),
            )
        for target, configuration in document.get("target", {}).items():
            for section in sections:
                inspect(
                    manifest,
                    f"target.{target}.{section}",
                    configuration.get(section, {}),
                )

    if not (ROOT / "Cargo.lock").is_file():
        messages.append("Cargo.lock is required")


def check_workflows(messages: list[str]) -> None:
    workflows = sorted((ROOT / ".github" / "workflows").glob("*.yml"))
    workflows += sorted((ROOT / ".github" / "workflows").glob("*.yaml"))
    if not workflows:
        messages.append("at least one GitHub Actions workflow is required")
    for workflow in workflows:
        text = workflow.read_text(encoding="utf-8")
        for action in unpinned_actions(text):
            messages.append(
                f"{workflow.relative_to(ROOT)}: action {action} is not pinned to a full SHA"
            )
        if "step-security/harden-runner@" not in text:
            messages.append(f"{workflow.relative_to(ROOT)}: Harden-Runner is required")
        if re.search(r"(?im)^\s*permissions:\s*write-all\s*$", text):
            messages.append(
                f"{workflow.relative_to(ROOT)}: broad write-all permissions are prohibited"
            )
        if re.search(r"(?im)^\s*continue-on-error:\s*true\s*$", text):
            messages.append(
                f"{workflow.relative_to(ROOT)}: security and quality gates cannot be ignored"
            )


def check_precommit(messages: list[str]) -> None:
    path = ROOT / ".pre-commit-config.yaml"
    if not path.is_file():
        messages.append(".pre-commit-config.yaml is required")
        return
    text = path.read_text(encoding="utf-8")
    for hook in (
        "spsp-policy",
        "policy-tests",
        "spsp-xml-assets",
        "rustfmt",
        "clippy",
        "tests",
        "gitleaks",
    ):
        if f"id: {hook}" not in text:
            messages.append(f"pre-commit must configure the {hook} hook")


def check_security_updates(messages: list[str]) -> None:
    path = ROOT / ".github" / "dependabot.yml"
    if not path.is_file():
        messages.append("Dependabot security-update configuration is required")
        return
    text = path.read_text(encoding="utf-8")
    for ecosystem in ("cargo", "github-actions"):
        if f"package-ecosystem: {ecosystem}" not in text:
            messages.append(f"Dependabot must monitor {ecosystem}")
    if text.count("open-pull-requests-limit: 0") < 2:
        messages.append(
            "Dependabot version updates must not create unbounded routine PRs"
        )
    if "versioning-strategy: increase" not in text:
        messages.append("Cargo Dependabot updates must preserve exact pins")


def main() -> int:
    messages: list[str] = []
    check_suppressions(messages)
    check_bypasses(messages)
    check_manifests(messages)
    check_workflows(messages)
    check_precommit(messages)
    check_security_updates(messages)
    return fail(messages)


if __name__ == "__main__":
    raise SystemExit(main())
