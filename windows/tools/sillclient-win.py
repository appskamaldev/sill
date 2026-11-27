#!/usr/bin/env python3
r"""Runs upstream Scripts/sillclient.py unchanged on Windows (and anywhere else).

Upstream hardcodes /usr/bin/openssl (identity, kind 18 check) and /usr/sbin/lsof, /bin/ps (the
synthetic-host check, which degrades safely without them). This wrapper loads the script's
source, swaps those absolute paths for the tools found here, and runs it with the same argv.
The upstream file is never edited.

  OpenSSL, first found: $SILL_OPENSSL; /usr/bin/openssl when it exists (macOS: the original,
           untouched); `openssl` on PATH; Git for Windows' C:\Program Files\Git\usr\bin\openssl.exe
           or C:\Program Files\Git\mingw64\bin\openssl.exe. None: the wrapper stops and says so.

usage: python windows\tools\sillclient-win.py PORT [seconds] [flags...]   (as sillclient.py)
"""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.normpath(os.path.join(HERE, "..", "..", "Scripts", "sillclient.py"))


GIT_OPENSSL = (r"C:\Program Files\Git\usr\bin\openssl.exe", r"C:\Program Files\Git\mingw64\bin\openssl.exe")


def tool(env, original, name, extra=()):
    if os.environ.get(env):
        return os.environ[env]
    if os.path.exists(original):
        return original
    found = shutil.which(name)
    if found:
        return found
    for p in extra:
        if os.path.exists(p):
            return p
    return None


def main():
    with open(SCRIPT, encoding="utf-8") as f:
        source = f.read()
    openssl = tool("SILL_OPENSSL", "/usr/bin/openssl", "openssl", GIT_OPENSSL)
    if openssl is None:
        sys.exit("sillclient-win: no OpenSSL found. Set SILL_OPENSSL to openssl.exe, e.g. "
                 "$env:SILL_OPENSSL = 'C:\\Program Files\\Git\\usr\\bin\\openssl.exe'")
    for original, path in (
        ("/usr/bin/openssl", openssl),
        # Without these the synthetic-host check just finds nothing (it degrades safely).
        ("/usr/sbin/lsof", tool("SILL_LSOF", "/usr/sbin/lsof", "lsof")),
        ("/bin/ps", tool("SILL_PS", "/bin/ps", "ps")),
    ):
        if path is not None and path != original:
            source = source.replace(f'"{original}"', repr(path))
    sys.argv[0] = SCRIPT
    globals_ = {"__name__": "__main__", "__file__": SCRIPT}
    exec(compile(source, SCRIPT, "exec"), globals_)


if __name__ == "__main__":
    main()
