#!/usr/bin/env python3
"""Run this isolated Cargo project with local, pinned Meson/Ninja build tools.

Usage: python3 build-wsl.py test --release --all-features --jobs 2
No pip, apt, administrator permission, or persistent environment changes.
"""

import hashlib
import io
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parent
LOCAL = ROOT / "target" / "build-tools"
WHEELS = (
    (
        "meson-1.12.1",
        "https://files.pythonhosted.org/packages/57/99/36cd25f9598eef70db97577440a70a360af505f6f01c2338a18d1556079c/meson-1.12.1-py3-none-any.whl",
        "930bc7542cbd9f57009e182fd014eba48cf1a0180a7b9006d2d32d8f168d8b02",
    ),
    (
        "ninja-1.13.2",
        "https://files.pythonhosted.org/packages/6e/53/ebfed7b689c338dd8ebeec9c0730c8d56821292f14e2536e5f3ef1a05744/ninja-1.13.2-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl",
        "65a24341b5ac09fcadcc37082660be40a94174e51a937fabf6e2cae26225fa2c",
    ),
)


def unpack(name: str, url: str, digest: str) -> Path:
    destination = LOCAL / name
    marker = destination / ".verified-sha256"
    if marker.is_file() and marker.read_text(encoding="ascii") == digest:
        return destination
    print(f"Preparing local build tool {name}", flush=True)
    with urllib.request.urlopen(url, timeout=60) as response:
        payload = response.read(32 * 1024 * 1024 + 1)
    if len(payload) > 32 * 1024 * 1024:
        raise ValueError(f"oversized wheel: {name}")
    if hashlib.sha256(payload).hexdigest() != digest:
        raise ValueError(f"wheel SHA256 mismatch: {name}")
    destination.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(io.BytesIO(payload)) as archive:
        for entry in archive.infolist():
            resolved = (destination / entry.filename).resolve()
            if not resolved.is_relative_to(destination.resolve()):
                raise ValueError("wheel path escapes extraction directory")
            if (entry.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("wheel contains a symbolic link")
        archive.extractall(destination)
    marker.write_text(digest, encoding="ascii")
    return destination


def main() -> int:
    if platform.system() != "Linux" or platform.machine() not in ("x86_64", "AMD64"):
        raise RuntimeError("this local build bootstrap is for Linux/WSL x86_64 only")
    if not sys.argv[1:]:
        raise ValueError("pass Cargo arguments, e.g. test --release --all-features --jobs 2")
    meson, ninja = (unpack(*wheel) for wheel in WHEELS)
    binary = LOCAL / "bin"
    binary.mkdir(parents=True, exist_ok=True)
    meson_script = binary / "meson"
    meson_script.write_text(
        "#!/usr/bin/env python3\nimport sys, runpy\n"
        f"sys.path.insert(0, {str(meson)!r})\n"
        # Upstream defaults to debugoptimized, independently of Cargo --release.
        # Make O3 and disabled debug assertions explicit for the comparison.
        "if len(sys.argv) > 1 and sys.argv[1] == 'setup':\n"
        "    sys.argv.extend(['--buildtype=release', '-Db_ndebug=true'])\n"
        "runpy.run_module('mesonbuild.mesonmain', run_name='__main__')\n",
        encoding="utf-8",
    )
    ninja_exe = ninja / "ninja-1.13.2.data" / "scripts" / "ninja"
    ninja_exe.chmod(0o755)
    ninja_script = binary / "ninja"
    ninja_script.write_text(
        f"#!/bin/sh\nexec {shlex.quote(str(ninja_exe))} -j2 \"$@\"\n",
        encoding="utf-8",
    )
    meson_script.chmod(0o755)
    ninja_script.chmod(0o755)
    environment = os.environ.copy()
    environment["PATH"] = f"{binary}:{Path.home() / '.cargo' / 'bin'}:{environment.get('PATH', '')}"
    environment.setdefault("CARGO_BUILD_JOBS", "2")
    environment.setdefault("CARGO_TARGET_DIR", str(ROOT / "target" / "cargo"))
    cargo = shutil.which("cargo", path=environment["PATH"])
    if cargo is None:
        raise RuntimeError("Cargo not found; install a supported toolchain explicitly")
    # The standalone workspace is selected by cwd; root Hark dependencies are untouched.
    return subprocess.run([cargo, *sys.argv[1:]], cwd=ROOT, env=environment, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
