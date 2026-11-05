"""Import generated SQL through the cf CLI in bounded batches.

Run against an empty remote D1 database after applying migrations. For local
Wrangler development data, use `wrangler d1 execute --local --file` instead.
"""

from __future__ import annotations

import argparse
import sqlite3
import subprocess
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_INPUT = REPO_ROOT / "data" / "pricebook.d1.sql"
MAX_BATCH_BYTES = 180_000


def read_statements(path: Path) -> list[str]:
    statements: list[str] = []
    pending = ""
    for line in path.read_text(encoding="utf-8").splitlines(keepends=True):
        pending += line
        if sqlite3.complete_statement(pending):
            statements.append(pending)
            pending = ""
    if pending.strip():
        raise ValueError("input ends with an incomplete SQL statement")
    return statements


def batches(statements: list[str]) -> list[str]:
    result: list[str] = []
    current = ""
    current_bytes = 0
    for statement in statements:
        size = len(statement.encode("utf-8"))
        if size > MAX_BATCH_BYTES:
            raise ValueError("one SQL statement exceeds the import batch limit")
        if current and current_bytes + size > MAX_BATCH_BYTES:
            result.append(current)
            current = ""
            current_bytes = 0
        current += statement
        current_bytes += size
    if current:
        result.append(current)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database_id")
    parser.add_argument("--input", type=Path, default=DEFAULT_INPUT)
    args = parser.parse_args()

    groups = batches(read_statements(args.input))
    for index, sql in enumerate(groups, start=1):
        command = ["cf", "d1", "query", args.database_id, f"--sql={sql}"]
        completed = subprocess.run(command, text=True, capture_output=True)
        if completed.returncode:
            print(completed.stdout, end="")
            print(completed.stderr, end="")
            raise SystemExit(f"D1 import failed in batch {index}/{len(groups)}")
        print(f"Imported batch {index}/{len(groups)}")


if __name__ == "__main__":
    main()
