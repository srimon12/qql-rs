from __future__ import annotations

import json
import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[3]
REPO_LOCAL_QQL_BIN = REPO_ROOT / ("qql-go.exe" if os.name == "nt" else "qql-go")


def resolve_qql_bin() -> str:
    override = os.environ.get("QQL_BIN")
    if override:
        return override

    path_bin = shutil.which("qql-go")
    if path_bin:
        return path_bin

    return str(REPO_LOCAL_QQL_BIN)


QQL_BIN = resolve_qql_bin()


@dataclass
class Result:
    message: str
    data: Any = None
    operation: str | None = None


def execute_json(query: str) -> Result:
    try:
        completed = subprocess.run(
            [QQL_BIN, "exec", "--quiet", "--json", query],
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as exc:
        raise RuntimeError(f"Unable to run qql-go binary at {QQL_BIN}") from exc

    stdout = completed.stdout.strip()
    stderr = completed.stderr.strip()

    if not stdout:
        raise RuntimeError(stderr or f"qql-go exited with code {completed.returncode}")

    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"qql-go did not return valid JSON: {stdout}") from exc

    if completed.returncode != 0 or not payload.get("ok", False):
        raise RuntimeError(payload.get("error") or stderr or "qql-go command failed")

    return Result(
        message=payload.get("message", ""),
        data=payload.get("data"),
        operation=payload.get("operation"),
    )


def drop_collection_if_exists(name: str) -> None:
    try:
        execute_json(f"DROP COLLECTION {name}")
    except Exception as exc:
        message = str(exc).lower()
        if "does not exist" in message or "not found" in message:
            return
        raise


def print_result(label: str, result: Result, limit: int = 5) -> None:
    print(f"[{label}] {result.message}")
    data = result.data
    if isinstance(data, list):
        for hit in data[:limit]:
            if isinstance(hit, dict):
                score = hit.get("score")
                hit_id = hit.get("id")
                print(f"  score={score} id={hit_id}")
            else:
                print(f"  {hit}")
    elif isinstance(data, dict):
        results = data.get("results")
        if isinstance(results, list):
            for hit in results[:limit]:
                score = hit.get("score")
                hit_id = hit.get("id")
                print(f"  score={score} id={hit_id}")
        elif data:
            print(f"  {data}")
    print()
