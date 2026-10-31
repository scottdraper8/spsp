"""Run the pinned actionlint binary from a verified, user-local cache."""

from __future__ import annotations

import hashlib
import os
import platform
import subprocess
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

VERSION = "1.7.12"
MAX_ARCHIVE_BYTES = 16 * 1024 * 1024
MAX_BINARY_BYTES = 32 * 1024 * 1024
ASSETS = {
    ("darwin", "amd64"): (
        f"actionlint_{VERSION}_darwin_amd64.tar.gz",
        "5b44c3bc2255115c9b69e30efc0fecdf498fdb63c5d58e17084fd5f16324c644",
        "actionlint",
    ),
    ("darwin", "arm64"): (
        f"actionlint_{VERSION}_darwin_arm64.tar.gz",
        "aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f",
        "actionlint",
    ),
    ("linux", "amd64"): (
        f"actionlint_{VERSION}_linux_amd64.tar.gz",
        "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8",
        "actionlint",
    ),
    ("linux", "arm64"): (
        f"actionlint_{VERSION}_linux_arm64.tar.gz",
        "325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6",
        "actionlint",
    ),
    ("windows", "amd64"): (
        f"actionlint_{VERSION}_windows_amd64.zip",
        "6e7241b51e6817ea6a047693d8e6fed13b31819c9a0dd6c5a726e1592d22f6e9",
        "actionlint.exe",
    ),
}


def platform_key() -> tuple[str, str]:
    system = platform.system().lower()
    machine = platform.machine().lower()
    architectures = {
        "x86_64": "amd64",
        "amd64": "amd64",
        "aarch64": "arm64",
        "arm64": "arm64",
    }
    architecture = architectures.get(machine)
    if system not in {"darwin", "linux", "windows"} or architecture is None:
        raise RuntimeError(f"actionlint {VERSION} is not pinned for {system}/{machine}")
    return system, architecture


def cache_root() -> Path:
    if platform.system() == "Windows":
        base = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local"))
    else:
        base = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    return base / "spsp" / "actionlint" / VERSION


def read_verified_archive(path: Path, expected_sha256: str) -> bytes | None:
    try:
        data = path.read_bytes()
    except FileNotFoundError:
        return None
    if (
        len(data) > MAX_ARCHIVE_BYTES
        or hashlib.sha256(data).hexdigest() != expected_sha256
    ):
        return None
    return data


def download_archive(url: str, destination: Path, expected_sha256: str) -> bytes:
    request = urllib.request.Request(
        url, headers={"User-Agent": f"SPSP/actionlint-{VERSION}"}
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        final_host = urllib.parse.urlparse(response.geturl()).hostname
        if final_host not in {"github.com", "release-assets.githubusercontent.com"}:
            raise RuntimeError("actionlint download redirected to an unapproved host")
        data = response.read(MAX_ARCHIVE_BYTES + 1)
    if (
        len(data) > MAX_ARCHIVE_BYTES
        or hashlib.sha256(data).hexdigest() != expected_sha256
    ):
        raise RuntimeError("actionlint release archive failed its pinned SHA-256 check")
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as temporary:
        temporary.write(data)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = Path(temporary.name)
    os.replace(temporary_path, destination)
    return data


def archive_binary(archive: bytes, asset_name: str, binary_name: str) -> bytes:
    if asset_name.endswith(".zip"):
        from io import BytesIO

        with zipfile.ZipFile(BytesIO(archive)) as bundle:
            member = next(
                (entry for entry in bundle.infolist() if entry.filename == binary_name),
                None,
            )
            if member is None or member.file_size > MAX_BINARY_BYTES:
                raise RuntimeError(
                    "actionlint binary is missing or exceeds the size limit"
                )
            return bundle.read(member)

    from io import BytesIO

    with tarfile.open(fileobj=BytesIO(archive), mode="r:gz") as bundle:
        member = next(
            (
                entry
                for entry in bundle.getmembers()
                if entry.name == binary_name and entry.isfile()
            ),
            None,
        )
        if member is None or member.size > MAX_BINARY_BYTES:
            raise RuntimeError("actionlint binary is missing or exceeds the size limit")
        stream = bundle.extractfile(member)
        if stream is None:
            raise RuntimeError("actionlint archive did not contain a readable binary")
        return stream.read(MAX_BINARY_BYTES + 1)


def ensure_binary() -> Path:
    key = platform_key()
    try:
        asset_name, expected_sha256, binary_name = ASSETS[key]
    except KeyError as error:
        raise RuntimeError(
            f"actionlint {VERSION} is not pinned for {key[0]}/{key[1]}"
        ) from error

    cache = cache_root()
    cache.mkdir(parents=True, exist_ok=True)
    archive_path = cache / asset_name
    archive = read_verified_archive(archive_path, expected_sha256)
    if archive is None:
        url = f"https://github.com/rhysd/actionlint/releases/download/v{VERSION}/{asset_name}"
        archive = download_archive(url, archive_path, expected_sha256)

    binary = archive_binary(archive, asset_name, binary_name)
    if len(binary) > MAX_BINARY_BYTES:
        raise RuntimeError("actionlint binary exceeds the size limit")
    executable = cache / binary_name
    with tempfile.NamedTemporaryFile(dir=cache, delete=False) as temporary:
        temporary.write(binary)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = Path(temporary.name)
    if os.name != "nt":
        temporary_path.chmod(0o755)
    os.replace(temporary_path, executable)
    return executable


def main() -> int:
    try:
        executable = ensure_binary()
    except (OSError, RuntimeError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(
            f"Unable to prepare pinned actionlint {VERSION}: {error}", file=sys.stderr
        )
        return 2
    return subprocess.run([str(executable), *sys.argv[1:]], check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
