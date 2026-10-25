#!/usr/bin/env python3
"""No secret has a literal default in the source.

    python3 scripts/secret_default_check.py [repo dir]

A variable whose name holds SECRET, KEY, TOKEN or PASSWORD (case-insensitive) must not fall back to a
non-empty literal when it is unset: such a default is a known value, so a deployment that forgets the
variable runs with it. Checked in the tracked files of the repository:

  Rust        env("NAME", "lit")    std::env::var("NAME").unwrap_or("lit" | ... "lit".into() | String::from("lit"))
  Python      os.environ.get("NAME", "lit")    os.getenv("NAME", "lit")    os.environ.get("NAME") or "lit"
  JS / TS     process.env.NAME || "lit"    process.env.NAME ?? "lit"    process.env["NAME"] || "lit"
  compose     ${NAME:-lit}    ${NAME-lit}

An empty default ("") is not reported here. Exit 1 on any finding, and also when no file was compared
(a check that reads nothing proves nothing).
"""
import re
import subprocess
import sys
from pathlib import Path

NAME = r"[A-Za-z0-9_]*(?:SECRET|KEY|TOKEN|PASSWORD)[A-Za-z0-9_]*"
LIT = r"""(?:"[^"\n]+"|'[^'\n]+')"""
PATTERNS = [
    ("rust env(name, lit)", re.compile(rf'\benv\(\s*"(?i:{NAME})"\s*,\s*"[^"\n]+"\s*\)')),
    (
        "rust var(name).unwrap_or(lit)",
        re.compile(
            rf'env::var\(\s*"(?i:{NAME})"\s*\)\s*\.(?:ok\(\)\s*\.)?unwrap_or(?:_else)?\(\s*(?:\|[^|]*\|\s*)?'
            rf'(?:String::from\(\s*)?"[^"\n]+"'
        ),
    ),
    ("python get(name, lit)", re.compile(rf'(?:environ\.get|getenv)\(\s*["\'](?i:{NAME})["\']\s*,\s*{LIT}')),
    ("python get(name) or lit", re.compile(rf'(?:environ\.get|getenv)\(\s*["\'](?i:{NAME})["\']\s*\)\s*or\s+{LIT}')),
    ("js env.name || lit", re.compile(rf'process\.env\.(?i:{NAME})\s*(?:\|\||\?\?)\s*{LIT}')),
    ("js env[name] || lit", re.compile(rf'process\.env\[\s*["\'](?i:{NAME})["\']\s*\]\s*(?:\|\||\?\?)\s*{LIT}')),
    ("compose ${{name:-lit}}", re.compile(rf'\$\{{(?i:{NAME}):?-[^}}\s]+\}}')),
]
SUFFIXES = {".rs", ".py", ".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx", ".yml", ".yaml", ".env", ".sh"}


def tracked(root: Path) -> list[Path]:
    out = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"], check=True, capture_output=True
    ).stdout.decode("utf-8")
    files = []
    for name in out.split("\0"):
        if not name:
            continue
        p = root / name
        if p.suffix in SUFFIXES or p.name.startswith(("docker-compose", ".env")):
            files.append(p)
    return files


def findings(text: str) -> list[tuple[int, str, str]]:
    found = []
    for lineno, line in enumerate(text.splitlines(), 1):
        for label, pat in PATTERNS:
            m = pat.search(line)
            if m:
                found.append((lineno, label, m.group(0)))
    return found


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    compared = 0
    bad = []
    for p in tracked(root):
        if p.name == "secret_default_check.py" or not p.is_file():
            continue
        try:
            text = p.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        compared += 1
        for lineno, label, hit in findings(text):
            bad.append(f"{p.relative_to(root)}:{lineno}: {label}: {hit}")
    for b in bad:
        print(f"secret_default_check: {b}")
    print(f"secret_default_check: compared {compared} file(s), {len(bad)} finding(s)")
    if compared == 0:
        print("secret_default_check: no file compared", file=sys.stderr)
        return 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
