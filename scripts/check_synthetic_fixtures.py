#!/usr/bin/env python3

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SYNTHETIC_ID_MIN = 900_000_000
SYNTHETIC_ID_MAX = 1_999_999_999

MOZILLA_EMAIL = re.compile(r"\b[A-Z0-9._%+-]+@mozilla\.(?:com|org)\b", re.IGNORECASE)
ISSUE_DASHBOARD_SLUG = re.compile(
    r"\bbug-\d{6,}---[a-z0-9][a-z0-9-]*\b", re.IGNORECASE
)
BUG_TAG = re.compile(r"^\s*-\s*bug\s+\d{6,}\b", re.IGNORECASE)
RESOURCE_ID = re.compile(
    r"(?<![A-Z0-9_])(?:id|query_id|dashboard_id|visualization_id|data_source_id|user_id)"
    r"[\"']?\s*:\s*(\d[\d_]*)\b",
    re.IGNORECASE,
)
RESOURCE_ID_CONSTANT = re.compile(
    r"\b[A-Z][A-Z0-9_]*_ID\s*:\s*u64\s*=\s*(\d[\d_]*)\b"
)
NUMERIC_CLI_ID = re.compile(
    r"\bstmo-cli\s+(?:fetch|execute|archive|unarchive|schedule|data-sources)\s+\d+\b"
    r"|\bstmo-cli\s+snippets\s+(?:fetch|delete)\s+\d+\b"
    r"|\bstmo-cli\s+execute\s+--data-source\s+\d+\b",
    re.IGNORECASE,
)


def find_violations(path: str, contents: str) -> list[tuple[int, str]]:
    violations = []
    for line_number, line in enumerate(contents.splitlines(), 1):
        if MOZILLA_EMAIL.search(line):
            violations.append((line_number, "use a fictional .invalid email address"))
        if ISSUE_DASHBOARD_SLUG.search(line):
            violations.append((line_number, "use a fictional dashboard slug"))
        if BUG_TAG.search(line):
            violations.append((line_number, "use a fictional tag instead of a live bug reference"))
        if NUMERIC_CLI_ID.search(line):
            violations.append((line_number, "use a placeholder for resource IDs in examples"))

        for match in (*RESOURCE_ID.finditer(line), *RESOURCE_ID_CONSTANT.finditer(line)):
            resource_id = int(match.group(1).replace("_", ""))
            if resource_id == 0:
                continue
            if not SYNTHETIC_ID_MIN <= resource_id <= SYNTHETIC_ID_MAX:
                violations.append(
                    (line_number, "use the reserved synthetic range for fixture resource IDs")
                )

    return violations


def repository_files() -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    paths = []
    for entry in result.stdout.split(b"\0"):
        if not entry:
            continue
        path = ROOT / Path(entry.decode("utf-8", errors="surrogateescape"))
        if path.is_file():
            paths.append(path)
    return paths


def check_repository() -> list[str]:
    failures = []
    for path in repository_files():
        try:
            contents = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue

        relative_path = path.relative_to(ROOT).as_posix()
        for line_number, reason in find_violations(relative_path, contents):
            failures.append(f"{relative_path}:{line_number}: {reason}")
    return failures


def run_self_test() -> None:
    synthetic_id = str(SYNTHETIC_ID_MIN)
    below_range_id = str(SYNTHETIC_ID_MIN - 1)
    fixture_email = "fixture" + "@" + "mozilla.com"
    issue_slug = "bug-" + str(2_006_698) + "---example-dashboard"
    numeric_example = "stmo-cli fetch " + str(123)

    cases = [
        ("id: " + synthetic_id + "\n", False),
        ("id: " + str(SYNTHETIC_ID_MAX) + "\n", False),
        ("const EXAMPLE_QUERY_ID: u64 = " + synthetic_id + ";\n", False),
        ("id: 0\n", False),
        ("id: " + below_range_id + "\n", True),
        ("const EXAMPLE_QUERY_ID: u64 = " + below_range_id + ";\n", True),
        (fixture_email, True),
        (issue_slug, True),
        (numeric_example, True),
    ]
    for contents, should_fail in cases:
        failed = bool(find_violations("fixture", contents))
        if failed != should_fail:
            raise AssertionError(f"source guard self-test failed for {contents!r}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        run_self_test()
        print("Synthetic-data guard self-test passed")
        return 0

    failures = check_repository()
    if failures:
        print("Production-derived STMO data checks failed:", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1

    print("No production-derived STMO identifiers found")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
