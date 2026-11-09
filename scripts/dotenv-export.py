#!/usr/bin/env python3
"""Copies chosen variables from a .env file into a key file for apb.

    dotenv-export.py <env-file> <out-file> NAME...

The .env file is read the way Nearbee reads it (dotenvy 0.15.7, ported
below): leading whitespace and `export`, quotes and escapes, inline comments
after whitespace, multi-line quoted values, `$NAME` and `${NAME}`
substitution (process environment first, then earlier lines), the last
assignment of a name wins, an empty value counts as not set. A file Nearbee
could not parse is refused. Each value is written as NAME="value" with
backslash, double quote, backtick and dollar escaped: systemd's
EnvironmentFile and a POSIX shell read that form alike.

Values are never printed: messages name variables and line numbers only.
Exit status: 0 done, 2 the .env file cannot be read or parsed, 64 usage.
"""
import os
import re
import sys

PREFIX = "apb-instance:"
NAME = re.compile(r"[A-Za-z_][A-Za-z0-9_]*\Z")


class ParseError(Exception):
    pass


def eval_end_state(state, chunk):
    """dotenvy's QuotedLines state machine over one physical line."""
    pos = 0
    for pos, c in enumerate(chunk):
        if state == "white":
            if c == "#":
                return pos, "comment"
            state = {"\\": "escape", '"': "weak", "'": "strong"}.get(c, "complete")
        elif state == "escape":
            state = "complete"
        elif state == "complete":
            if c.isspace() and c not in "\n\r":
                state = "white"
            else:
                state = {"\\": "escape", '"': "weak", "'": "strong"}.get(c, "complete")
        elif state == "weak":
            state = {"\\": "weak_escape", '"': "complete"}.get(c, "weak")
        elif state == "weak_escape":
            state = "weak"
        elif state == "strong":
            state = {"\\": "strong_escape", "'": "complete"}.get(c, "strong")
        elif state == "strong_escape":
            state = "strong"
    return pos, state


def logical_lines(text):
    """Yields (line number, logical line) as dotenvy's QuotedLines does."""
    parts = text.split("\n")
    physical = [p + "\n" for p in parts[:-1]]
    if parts[-1]:
        physical.append(parts[-1])
    i = 0
    while i < len(physical):
        start = i + 1
        buf = ""
        state = "complete"
        while True:
            if i == len(physical):
                raise ParseError(start)
            buf_pos = len(buf)
            buf += physical[i]
            i += 1
            if buf.lstrip().startswith("#"):
                yield start, ""
                break
            pos, state = eval_end_state(state, buf[buf_pos:])
            if state == "complete":
                if buf.endswith("\n"):
                    buf = buf[:-1]
                    if buf.endswith("\r"):
                        buf = buf[:-1]
                yield start, buf
                break
            if state == "comment":
                yield start, buf[: buf_pos + pos]
                break


def parse_key(s, lineno):
    if not s or not (s[0].isascii() and (s[0].isalpha() or s[0] == "_")):
        raise ParseError(lineno)
    end = 0
    while end < len(s) and s[end].isascii() and (s[end].isalnum() or s[end] in "_."):
        end += 1
    return s[:end], s[end:]


def substitute(name, data, out):
    if name in os.environ:
        out.append(os.environ[name])
    else:
        out.append(data.get(name) or "")


def parse_value(s, data, lineno):
    strong = weak = escaped = expecting_end = False
    out, name = [], []
    mode = None  # None, "block" ($NAME) or "braced" (${NAME})
    for c in s:
        if expecting_end:
            if c in " \t":
                continue
            if c == "#":
                break
            raise ParseError(lineno)
        elif escaped:
            if c in "\\'\"$ ":
                out.append(c)
            elif c == "n":
                out.append("\n")
            else:
                raise ParseError(lineno)
            escaped = False
        elif strong:
            if c == "'":
                strong = False
            else:
                out.append(c)
        elif mode is not None:
            if c.isalnum():
                name.append(c)
            elif mode == "block":
                if c == "{" and not name:
                    mode = "braced"
                else:
                    substitute("".join(name), data, out)
                    name = []
                    if c == "$":
                        mode = "block"
                    else:
                        mode = None
                        out.append(c)
            elif c == "}":
                mode = None
                substitute("".join(name), data, out)
                name = []
            else:
                name.append(c)
        elif c == "$":
            mode = "block"
        elif weak:
            if c == '"':
                weak = False
            elif c == "\\":
                escaped = True
            else:
                out.append(c)
        elif c == "'":
            strong = True
        elif c == '"':
            weak = True
        elif c == "\\":
            escaped = True
        elif c in " \t":
            expecting_end = True
        else:
            out.append(c)
    if mode == "braced" or strong or weak:
        raise ParseError(lineno)
    substitute("".join(name), data, out)
    return "".join(out)


def parse(text):
    """Every assignment in order, as dotenvy's iterator yields them."""
    data = {}
    values = {}
    for lineno, line in logical_lines(text):
        s = line.rstrip().lstrip()
        if not s or s.startswith("#"):
            continue
        key, s = parse_key(s, lineno)
        s = s.lstrip()
        if key == "export" and not s.startswith("="):
            key, s = parse_key(s, lineno)
            s = s.lstrip()
        if not s.startswith("="):
            raise ParseError(lineno)
        s = s[1:].lstrip()
        if not s or s.startswith("#"):
            data[key] = None
            values[key] = ""
            continue
        value = parse_value(s, data, lineno)
        data[key] = value
        values[key] = value
    return values


def quoted(value):
    for c in "\\\"`$":
        value = value.replace(c, "\\" + c)
    return '"' + value + '"'


def main(argv):
    if len(argv) < 3:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 64
    env_file, out_file, names = argv[0], argv[1], argv[2:]
    for n in names:
        if not NAME.match(n):
            print(f"{PREFIX} {n!r} is not a variable name", file=sys.stderr)
            return 64
    try:
        with open(env_file, encoding="utf-8", newline="") as f:
            values = parse(f.read())
    except ParseError as e:
        print(f"{PREFIX} cannot parse {env_file} at line {e.args[0]}", file=sys.stderr)
        return 2
    except (OSError, UnicodeDecodeError) as e:
        print(f"{PREFIX} cannot read {env_file}: {type(e).__name__}", file=sys.stderr)
        return 2
    lines = []
    for n in names:
        v = values.get(n, "")
        if v and "\0" not in v:
            lines.append(f"{n}={quoted(v)}\n")
            print(f"{PREFIX} passing {n} to the instance")
        else:
            print(f"{PREFIX} {n} is not set in {env_file}")
    with open(out_file, "w", encoding="utf-8") as f:
        f.writelines(lines)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
