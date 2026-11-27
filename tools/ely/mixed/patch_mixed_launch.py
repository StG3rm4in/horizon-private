"""Set the deployment's mixed address in the Prism wrapper and ely-play patch.

Usage:
    python3 -I patch_mixed_launch.py SERVER_ROOT ELY_LAUNCHER_PATCH HOST:PORT

SERVER_ROOT contains client/prism/wrapper/src/main.rs. Apply
p_wrapper_cosmetics.py to the wrapper first. Use a deployment copy of
patch_ely_launcher.py and apply that copy to the Horizon checkout afterwards.
"""

import ipaddress
import os
from pathlib import Path
import re
import stat
import sys
import tempfile


def address(text):
    host, separator, port = text.rpartition(":")
    if not separator or not re.fullmatch(r"[0-9]{1,5}", port) or not 1 <= int(port) <= 65535:
        raise ValueError("mixed server address must include a valid numeric port")
    if host.startswith("[") and host.endswith("]") and "%" not in host:
        host = f"[{ipaddress.IPv6Address(host[1:-1])}]"
    elif not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9.-]{0,251}[A-Za-z0-9.])?", host):
        raise ValueError("mixed server host must be ASCII DNS, IPv4, or bracketed IPv6")
    return f"{host.lower()}:{int(port)}"


def staged_copy(path, content, mode):
    fd, name = tempfile.mkstemp(dir=path.parent, prefix=".mixed-launch-")
    temp = Path(name)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(content)
        temp.chmod(mode)
        return temp
    except BaseException:
        temp.unlink(missing_ok=True)
        raise


def extend_launcher_test(path, content, newline, host):
    cosmetics = (
        '        assert_eq!(overrides.get(OsStr::new("HORIZON_COSMETICS")), '
        'Some(&Some(OsStr::new("0"))));'
    )
    removals = (
        '        for name in ["HORIZON_COSMETICS_API", "HORIZON_COSMETICS_LOCAL", '
        '"HORIZON_SESSION_FILE"] {'
    )
    anchor = newline.join((cosmetics.encode(), removals.encode(), b""))
    replacement = newline.join((
        cosmetics.encode(),
        b'        assert_eq!(overrides.get(OsStr::new("HORIZON_MIXED_CLIENT")),',
        f'            Some(&Some(OsStr::new("{host}"))));'.encode(),
        removals.replace('"HORIZON_SESSION_FILE"]',
                         '"HORIZON_SESSION_FILE", "HORIZON_MIXED_PROVIDERS"]').encode(),
        b"",
    ))
    if content.count(anchor) != 1:
        raise ValueError(f"{path}: expected one original ely-play environment test")
    return content.replace(anchor, replacement, 1)


def main():
    if len(sys.argv) != 4:
        raise ValueError(f"usage: {Path(sys.argv[0]).name} SERVER_ROOT ELY_LAUNCHER_PATCH HOST:PORT")
    _, root, launcher, host = sys.argv
    host = address(host)
    wrapper = Path(root) / "client/prism/wrapper/src/main.rs"
    inputs = ((wrapper, False), (Path(launcher), True))
    changes = {}
    for path, is_launcher in inputs:
        path = path.resolve(strict=True)
        original = path.read_bytes()
        newline = b"\r\n" if b"\r\n" in original else b"\n"
        anchor = b'        .env("HORIZON_COSMETICS", "0")' + newline
        remove_engine = b'        .env_remove("HORIZON_MIXED_PROVIDERS")' + newline
        addition = f'        .env("HORIZON_MIXED_CLIENT", "{host}")'.encode() + newline
        if b"HORIZON_MIXED_CLIENT" in original or original.count(anchor) != 1:
            raise ValueError(f"{path}: expected one original cosmetics setting and no mixed setting")
        revised = original.replace(anchor, anchor + remove_engine + addition, 1)
        if is_launcher:
            revised = extend_launcher_test(path, revised, newline, host)
        changes[path] = (original, revised)

    staged = {}
    backups = {}
    installed = []
    try:
        for path, (original, revised) in changes.items():
            mode = stat.S_IMODE(path.stat().st_mode)
            staged[path] = staged_copy(path, revised, mode)
            backups[path] = staged_copy(path, original, mode)
        for path, (original, _) in changes.items():
            if path.read_bytes() != original:
                raise ValueError(f"{path}: changed during preflight")
        for path, temp in staged.items():
            os.replace(temp, path)
            installed.append(path)
    except BaseException:
        for path in reversed(installed):
            os.replace(backups[path], path)
        raise
    finally:
        for temp in (*staged.values(), *backups.values()):
            temp.unlink(missing_ok=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        raise SystemExit(str(error)) from None
