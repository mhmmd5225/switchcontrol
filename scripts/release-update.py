"""Create auto-update assets from the two platform release archives."""

import hashlib
import os
import re
import shutil
import zipfile
from pathlib import Path

version = os.environ["SWITCHCONTROL_VERSION"]
if not re.fullmatch(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version):
    raise SystemExit(f"Expected a stable release tag, got {version!r}")

windows = Path("switchcontrol.exe")
with zipfile.ZipFile("SwitchControl-Windows.zip") as archive:
    with archive.open("switchcontrol.exe") as source, windows.open("wb") as target:
        shutil.copyfileobj(source, target)

manifest = [f'version = "{version}"']
for platform, asset in (("windows", windows), ("macos", Path("SwitchControl-macOS.zip"))):
    with asset.open("rb") as payload:
        digest = hashlib.file_digest(payload, "sha256").hexdigest()
    manifest.extend(["", f"[{platform}]", f'sha256 = "{digest}"', f"bytes = {asset.stat().st_size}"])

Path("update.toml").write_text("\n".join(manifest) + "\n", encoding="utf-8")
