"""The workspace writes of `scripts/apb-instance.sh install`.

    python3 apb-workspace-write.py <workspace>   (payload as JSON on stdin)

The payload is {"files": {"<relative path>": "<text>", ...}, "decisions_off":
true}. Every file is written under <workspace> and `decisions: { enabled:
false }` is set in `.apb/config.yaml`, keeping its other keys.

In sandbox mode this runs inside the container (`python3 -c`), so it reaches
only what the container mounts. In either mode it never follows a symlink
below the workspace: every directory is opened relative to its parent with
O_NOFOLLOW, every file is written to a fresh random name with O_EXCL and
moved into place by rename, which replaces a link instead of writing through
it. A sandboxed step writes the workspace under the same uid, so a path
checked first and opened later could be swapped in between.
"""

import json
import os
import re
import secrets
import sys

DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC


def die(msg):
    print(f"apb-instance: {msg}", file=sys.stderr)
    sys.exit(1)


def open_dir(root_fd, parts, shown):
    """The directory `parts` below root_fd, made when missing; never a link."""
    fd = os.dup(root_fd)
    walked = []
    for part in parts:
        walked.append(part)
        try:
            os.mkdir(part, 0o755, dir_fd=fd)
        except FileExistsError:
            pass
        try:
            nfd = os.open(part, DIR_FLAGS, dir_fd=fd)
        except OSError as e:
            os.close(fd)
            die(f"{shown}/{'/'.join(walked)} is a symlink or not a directory ({e.strerror}); nothing written there")
        os.close(fd)
        fd = nfd
    return fd


def write_at(dir_fd, name, data):
    tmp = f".nearbee-{secrets.token_hex(8)}"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o644, dir_fd=dir_fd)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(data)
        os.replace(tmp, name, src_dir_fd=dir_fd, dst_dir_fd=dir_fd)
    except BaseException:
        try:
            os.unlink(tmp, dir_fd=dir_fd)
        except OSError:
            pass
        raise


def read_at(dir_fd, name, shown):
    try:
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=dir_fd)
    except FileNotFoundError:
        return ""
    except OSError as e:
        die(f"{shown} is a symlink or cannot be read ({e.strerror}); not rewritten")
    with os.fdopen(fd, encoding="utf-8") as f:
        return f.read()


def decisions_off(text):
    """`text` with any top-level decisions block replaced by enabled: false."""
    out, skip = [], False
    for line in text.splitlines():
        if re.match(r"^decisions\s*:", line):
            skip = True
            continue
        if skip and (line.startswith((" ", "\t")) or not line.strip()):
            continue
        skip = False
        out.append(line)
    out += ["decisions:", "  enabled: false"]
    return "\n".join(out) + "\n"


def main():
    ws = sys.argv[1]
    payload = json.load(sys.stdin)
    root = os.open(ws, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    for rel, text in sorted(payload.get("files", {}).items()):
        parts = rel.split("/")
        if any(p in ("", ".", "..") for p in parts):
            die(f"unsafe path {rel}")
        d = open_dir(root, parts[:-1], ws)
        try:
            write_at(d, parts[-1], text)
        finally:
            os.close(d)
    if payload.get("decisions_off"):
        d = open_dir(root, [".apb"], ws)
        try:
            shown = f"{ws}/.apb/config.yaml"
            write_at(d, "config.yaml", decisions_off(read_at(d, "config.yaml", shown)))
        finally:
            os.close(d)
    os.close(root)


if __name__ == "__main__":
    main()
