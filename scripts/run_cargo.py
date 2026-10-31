"""Run the pinned project Cargo toolchain when cargo is outside the ambient PATH."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def cargo_executable() -> Path:
    configured = os.environ.get("CARGO")
    if configured:
        found = shutil.which(configured)
        if found is not None:
            return Path(found)
        candidate = Path(configured).expanduser()
        if candidate.is_file():
            return candidate
        raise FileNotFoundError(
            "the CARGO environment variable does not name an executable"
        )

    found = shutil.which("cargo")
    if found is not None:
        return Path(found)

    rustup_proxy = (
        Path.home() / ".cargo" / "bin" / ("cargo.exe" if os.name == "nt" else "cargo")
    )
    if rustup_proxy.is_file():
        return rustup_proxy
    raise FileNotFoundError("Cargo is unavailable; install the pinned Rust toolchain")


def main() -> int:
    try:
        executable = cargo_executable()
    except FileNotFoundError as error:
        print(f"SPSP Rust check failed: {error}", file=sys.stderr)
        return 127
    return subprocess.run(
        [str(executable), *sys.argv[1:]], cwd=ROOT, check=False
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
