#!/usr/bin/env python3
"""Validate and synchronize the QQL release metadata.

Check mode (default — used by CI and the release gate):

    python3 scripts/check_release.py
    python3 scripts/check_release.py --version 0.4.0

Update modes (single source of truth is the VERSION file; every registry
manifest is rewritten from it, then the result is re-validated):

    python3 scripts/check_release.py set 0.4.0 [--dry-run]
    python3 scripts/check_release.py bump major|minor|patch [--dry-run]

Version sites owned by this script:

- ``VERSION`` (root source of truth)
- root ``Cargo.toml``: ``[workspace.package].version`` and the five internal
  ``[workspace.dependencies]`` path entries
- crate manifests: internal path dependencies that pin a literal version
  (``qql-conformance``, ``qql-wasm``, …)
- ``crates/{pyqql,pyqql-edge}/pyproject.toml`` ``[project].version``
- ``crates/{nqql,nqql-edge}/package.json`` ``version`` plus every
  ``optionalDependencies`` platform package

Deliberately NOT rewritten: generated wasm bundles (``editors/vscode/wasm``,
``crates/qql-wasm/pkg`` — rebuilt with wasm-pack), ``Cargo.lock`` (refreshed
via ``cargo update -w``), the VS Code extension ``package.json`` (packaging
slot with an independent version), and prose files (``CHANGELOG.md``,
``editors/vscode/README.md``, ``bench/README.md`` — printed as reminders).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION_FILE = ROOT / "VERSION"
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+(?:[-.][0-9A-Za-z.-]+)?$")

PUBLIC_CRATES = (
    "qql-core",
    "qql-plan",
    "qql-embed",
    "qql-runtime",
    "qql-edge",
    "qql-cli",
)
PRIVATE_CRATES = (
    "qql-conformance",
    "qql-grammar-gen",
    "qql-wasm",
    "pyqql",
    "pyqql-edge",
    "nqql",
    "nqql-edge",
)
ALL_CRATES = PUBLIC_CRATES + PRIVATE_CRATES
# Internal crates that appear as versioned path dependencies in manifests.
INTERNAL_DEP_KEYS = ("qql-core", "qql-plan", "qql-embed", "qql", "qql-edge")
PYTHON_PACKAGES = ("pyqql", "pyqql-edge")
NODE_PACKAGES = ("nqql", "nqql-edge")


def fail(message: str) -> None:
    print(f"release metadata error: {message}", file=sys.stderr)
    raise SystemExit(1)


def warn(message: str) -> None:
    print(f"release metadata warning: {message}")


def load_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def package_version(package: dict, workspace_version: str) -> str:
    value = package.get("version")
    if isinstance(value, dict) and value.get("workspace") is True:
        return workspace_version
    if isinstance(value, str):
        return value
    fail("Cargo package does not declare a version")


def normalize_version(raw: str) -> str:
    value = raw.strip()
    if value.startswith("v"):
        value = value[1:]
    if not SEMVER_RE.match(value):
        fail(f"invalid release version {raw!r} (expected semver like 1.2.3)")
    return value


def read_expected_version(explicit: str | None) -> str:
    if explicit:
        return normalize_version(explicit)
    if VERSION_FILE.is_file():
        return normalize_version(VERSION_FILE.read_text())
    root = load_toml(ROOT / "Cargo.toml")
    return normalize_version(root["workspace"]["package"]["version"])


def workspace_version() -> str:
    root = load_toml(ROOT / "Cargo.toml")
    return root["workspace"]["package"]["version"]


# --------------------------------------------------------------------------
# Check mode
# --------------------------------------------------------------------------


def validate_cargo(expected: str) -> None:
    root = load_toml(ROOT / "Cargo.toml")
    workspace_package = root["workspace"]["package"]
    workspace_v = workspace_package["version"]
    if workspace_v != expected:
        fail(f"workspace version is {workspace_v}, expected release {expected}")
    for key in ("authors", "license", "repository", "homepage", "rust-version"):
        if not workspace_package.get(key):
            fail(f"workspace.package.{key} is missing")

    deps = root.get("workspace", {}).get("dependencies", {})
    for key in INTERNAL_DEP_KEYS:
        spec = deps.get(key)
        if not isinstance(spec, dict) or spec.get("version") != expected:
            fail(f"workspace.dependencies.{key} must declare version {expected}")
        if not isinstance(spec.get("path"), str):
            fail(f"workspace.dependencies.{key} must be a path dependency")

    for crate in ALL_CRATES:
        manifest = ROOT / "crates" / crate / "Cargo.toml"
        data = load_toml(manifest)
        package = data["package"]
        actual = package_version(package, expected)
        if actual != expected:
            fail(f"{manifest.relative_to(ROOT)} has version {actual}, expected {expected}")

        should_publish = crate in PUBLIC_CRATES
        if should_publish and package.get("publish") is False:
            fail(f"{crate} must be publishable on crates.io")
        if not should_publish and package.get("publish") is not False:
            fail(f"{crate} must set publish = false")
        if should_publish:
            crate_license = manifest.parent / "LICENSE"
            root_license = ROOT / "LICENSE"
            if (
                not crate_license.is_file()
                or crate_license.read_bytes() != root_license.read_bytes()
            ):
                fail(f"{crate}/LICENSE must match the root LICENSE")

        if not package.get("description"):
            fail(f"{crate} is missing a package description")
        for key in ("authors", "license", "repository", "homepage", "rust-version"):
            inherited = package.get(key)
            if not (isinstance(inherited, dict) and inherited.get("workspace") is True):
                fail(f"{crate} must inherit package.{key} from the workspace")

        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            for dependency, spec in data.get(section, {}).items():
                if not isinstance(spec, dict) or "path" not in spec:
                    continue
                if dependency.startswith("qql") and spec.get("version") != expected:
                    fail(
                        f"{crate} {section}.{dependency} must declare version {expected}"
                    )


def validate_python(expected: str) -> None:
    root_license = (ROOT / "LICENSE").read_bytes()
    for package in PYTHON_PACKAGES:
        path = ROOT / "crates" / package / "pyproject.toml"
        project = load_toml(path)["project"]
        if project.get("version") != expected:
            fail(f"{package} Python version must be {expected}")
        if project.get("license") != "MIT":
            fail(f"{package} Python license must be MIT")
        if not project.get("authors") or not project.get("urls"):
            fail(f"{package} is missing Python author or project URLs")
        package_license = ROOT / "crates" / package / "LICENSE"
        if not package_license.is_file() or package_license.read_bytes() != root_license:
            fail(f"{package}/LICENSE must match the root LICENSE")


def validate_node(expected: str) -> None:
    root_license = (ROOT / "LICENSE").read_bytes()
    for package in NODE_PACKAGES:
        path = ROOT / "crates" / package / "package.json"
        data = json.loads(path.read_text())
        if data.get("version") != expected:
            fail(f"{package} npm version must be {expected}")
        if data.get("license") != "MIT":
            fail(f"{package} npm license must be MIT")
        if not data.get("repository") or not data.get("author"):
            fail(f"{package} is missing npm author or repository metadata")
        if "README.md" not in data.get("files", []):
            fail(f"{package} npm files must include README.md")
        for dependency, version in data.get("optionalDependencies", {}).items():
            if version != expected:
                fail(f"{package} optional dependency {dependency} must be {expected}")
        package_license = path.with_name("LICENSE")
        if not package_license.is_file() or package_license.read_bytes() != root_license:
            fail(f"{package}/LICENSE must match the root LICENSE")


def validate_editor(expected: str) -> None:
    """The VS Code extension version is intentionally independent from the
    workspace version (packaging slot), but it must exist and its bundled WASM
    copy must declare the same version."""
    ext = json.loads((ROOT / "editors" / "vscode" / "package.json").read_text())
    if not isinstance(ext.get("version"), str) or not ext["version"]:
        fail("editors/vscode/package.json is missing a version")
    if ext["version"] != expected:
        warn(
            f"VS Code extension version is {ext['version']}, workspace release is "
            f"{expected}; the extension version is intentionally independent — "
            "bump editors/vscode/package.json only if the extension itself changed"
        )
    bundled = ROOT / "editors" / "vscode" / "wasm" / "package.json"
    if not bundled.is_file():
        fail("editors/vscode/wasm/package.json is missing (bundled editor WASM)")
    wasm_pkg = json.loads(bundled.read_text())
    if wasm_pkg.get("name") != "qql-wasm":
        fail("editors/vscode/wasm/package.json must be the qql-wasm bundle")
    if not wasm_pkg.get("version"):
        fail("editors/vscode/wasm/package.json is missing a version")
    if wasm_pkg.get("version") != expected:
        warn(
            f"bundled editor WASM reports version {wasm_pkg['version']}, workspace "
            f"release is {expected}; rebuild with wasm-pack (target nodejs) before "
            "shipping the extension"
        )
    for export in ("formatQuery",):
        d_ts = ROOT / "editors" / "vscode" / "wasm" / "qql_wasm.d.ts"
        if not d_ts.is_file():
            fail("editors/vscode/wasm/qql_wasm.d.ts is missing (stale bundle?)")
        if export not in d_ts.read_text():
            fail(
                f"bundled editor WASM does not export {export}; "
                "rebuild with wasm-pack from crates/qql-wasm"
            )


def run_checks(expected: str) -> None:
    if not (ROOT / "LICENSE").is_file():
        fail("root MIT LICENSE is missing")
    if not (ROOT / "crates" / "qql-runtime" / "openapi.json").is_file():
        fail("qql-runtime/openapi.json is missing from the publishable crate")
    wasm_license = ROOT / "crates" / "qql-wasm" / "LICENSE"
    if (
        not wasm_license.is_file()
        or wasm_license.read_bytes() != (ROOT / "LICENSE").read_bytes()
    ):
        fail("qql-wasm/LICENSE must match the root LICENSE")
    if VERSION_FILE.is_file():
        file_version = VERSION_FILE.read_text().strip()
        if file_version != expected:
            fail(f"VERSION file is {file_version!r}, expected release {expected}")

    validate_cargo(expected)
    validate_python(expected)
    validate_node(expected)
    validate_editor(expected)


# --------------------------------------------------------------------------
# Update mode
# --------------------------------------------------------------------------


def replace_workspace_package_version(text: str, old: str, new: str) -> tuple[str, int]:
    lines = text.splitlines(keepends=True)
    inside = False
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            inside = stripped == "[workspace.package]"
            continue
        if inside and re.search(rf'^version\s*=\s*"{re.escape(old)}"\s*$', stripped):
            lines[index] = line.replace(f'"{old}"', f'"{new}"', 1)
            return "".join(lines), 1
    return text, 0


def replace_internal_dep_versions(text: str, old: str, new: str) -> tuple[str, int]:
    """Rewrite `version = "old"` on internal path-dependency lines only.

    A line qualifies when it declares a dependency table entry with a `path`
    pointing into the workspace (`"crates/..."` or `"../crate"`). Third-party
    dependency versions are never touched.
    """
    pattern = re.compile(rf'(version\s*=\s*)"{re.escape(old)}"')
    out: list[str] = []
    count = 0
    for line in text.splitlines(keepends=True):
        if "path =" in line and ('"crates/' in line or '"../' in line):
            line, hits = pattern.subn(rf'\g<1>"{new}"', line)
            count += hits
        out.append(line)
    return "".join(out), count


def replace_pyproject_version(text: str, old: str, new: str) -> tuple[str, int]:
    """Rewrite `[project].version` only (first `version = "old"` after the
    `[project]` header, before the next table header)."""
    marker = text.find("[project]")
    if marker == -1:
        return text, 0
    next_table = text.find("\n[", marker + 1)
    end = next_table if next_table != -1 else len(text)
    needle = f'version = "{old}"'
    segment = text[marker:end]
    if needle not in segment:
        return text, 0
    segment = segment.replace(needle, f'version = "{new}"', 1)
    return text[:marker] + segment + text[end:], 1


def update_node_package(path: Path, old: str, new: str) -> tuple[str, list[str]]:
    data = json.loads(path.read_text())
    changes: list[str] = []
    if data.get("version") == old:
        data["version"] = new
        changes.append("version")
    optional = data.get("optionalDependencies", {})
    for name in list(optional):
        if optional[name] == old:
            optional[name] = new
            changes.append(f"optionalDependencies.{name}")
    if not changes:
        return path.read_text(), []
    return json.dumps(data, indent=2) + "\n", changes


def collect_update_plan(old: str, new: str) -> list[tuple[Path, str, str]]:
    """Return [(path, description, new_content)] for every version site."""
    plan: list[tuple[Path, str, str]] = []

    plan.append((VERSION_FILE, "VERSION (source of truth)", new + "\n"))

    root_manifest = ROOT / "Cargo.toml"
    root_text = root_manifest.read_text()
    text, hits = replace_workspace_package_version(root_text, old, new)
    if hits != 1:
        fail(f'root Cargo.toml [workspace.package] has no version = "{old}" line')
    text, dep_hits = replace_internal_dep_versions(text, old, new)
    if dep_hits != len(INTERNAL_DEP_KEYS):
        fail(
            f"root Cargo.toml [workspace.dependencies] declares {dep_hits} of "
            f"{len(INTERNAL_DEP_KEYS)} internal path deps at {old}"
        )
    plan.append((root_manifest, "workspace.package version + 5 workspace.dependencies", text))

    for crate in ALL_CRATES:
        manifest = ROOT / "crates" / crate / "Cargo.toml"
        if not manifest.is_file():
            continue
        text = manifest.read_text()
        new_text, hits = replace_internal_dep_versions(text, old, new)
        if hits:
            plan.append((manifest, f"{hits} internal path dep(s)", new_text))

    for package in PYTHON_PACKAGES:
        path = ROOT / "crates" / package / "pyproject.toml"
        text, hits = replace_pyproject_version(path.read_text(), old, new)
        if hits != 1:
            fail(f'{package} pyproject.toml [project] has no version = "{old}" line')
        plan.append((path, "[project].version", text))

    for package in NODE_PACKAGES:
        path = ROOT / "crates" / package / "package.json"
        text, changes = update_node_package(path, old, new)
        if not changes:
            fail(f"{package} package.json has no version/optionalDependencies at {old}")
        plan.append((path, ", ".join(changes), text))

    return plan


def refresh_lockfile() -> None:
    """Sync workspace package versions into Cargo.lock without touching
    third-party dependency versions."""
    try:
        result = subprocess.run(
            ["cargo", "update", "-w", "--offline"],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            result = subprocess.run(
                ["cargo", "update", "-w"], cwd=ROOT, capture_output=True, text=True
            )
        if result.returncode != 0:
            tail = result.stderr.strip().splitlines()[-1] if result.stderr else "unknown"
            warn(f"could not refresh Cargo.lock automatically ({tail}); run `cargo update -w` manually")
    except FileNotFoundError:
        warn("cargo not found; run `cargo update -w` to refresh Cargo.lock")


def print_reminders(old: str, new: str) -> None:
    print()
    print("Manual follow-ups (not automated):")
    print(
        "  1. Rebuild the bundled editor WASM so it ships the new parser:\n"
        "       wasm-pack build crates/qql-wasm --release --target nodejs \\\n"
        "         --out-dir ../../editors/vscode/wasm\n"
        "     (target nodejs is required — a bundler target breaks the extension host)"
    )
    print(
        f"  2. CHANGELOG.md: move the [Unreleased] notes into a [{new}] section "
        "dated today."
    )
    print(
        "  3. Prose version mentions to review: editors/vscode/README.md, "
        "bench/README.md (historical), website installation docs."
    )
    print(
        "  4. Release flow (RELEASING.md): PR to dev -> CI green -> merge dev into "
        f"main -> tag v{new} on the merged commit (only the tag publishes)."
    )


def apply_version(old: str, new: str, dry_run: bool) -> None:
    plan = collect_update_plan(old, new)
    if dry_run:
        print(f"dry run: {len(plan)} files would change ({old} -> {new}):")
        for path, description, _ in plan:
            print(f"  - {path.relative_to(ROOT)}  ({description})")
        print("dry run: no files written, Cargo.lock untouched")
        return

    for path, description, content in plan:
        path.write_text(content)
        print(f"updated {path.relative_to(ROOT)}  ({description})")

    refresh_lockfile()

    run_checks(new)
    print()
    print(f"release metadata synchronized to {new} and re-validated")
    print_reminders(old, new)


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Validate (default) or synchronize the QQL release metadata."
    )
    parser.add_argument(
        "--version",
        help="Check mode: expected release version (defaults to the VERSION file).",
    )
    parser.add_argument(
        "command",
        nargs="?",
        choices=("set", "bump", "check"),
        help="Update mode: `set <version>` or `bump major|minor|patch`; omit for check.",
    )
    parser.add_argument(
        "value",
        nargs="?",
        help="For `set`: the target version. For `bump`: major, minor, or patch.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="With set/bump: print the plan without writing anything.",
    )
    args = parser.parse_args()

    if args.command in ("set", "bump"):
        if args.version:
            fail("--version is only valid in check mode (omit the command)")
        current = read_expected_version(None)
        if args.command == "set":
            target = normalize_version(args.value or "")
        else:
            if args.value not in ("major", "minor", "patch"):
                fail("bump requires a level: major, minor, or patch")
            release_part = current.split("-", 1)[0]
            major, minor, patch = (int(part) for part in release_part.split("."))
            if args.value == "major":
                major, minor, patch = major + 1, 0, 0
            elif args.value == "minor":
                minor, patch = minor + 1, 0
            else:
                patch += 1
            target = f"{major}.{minor}.{patch}"
        if current == target:
            print(f"already at {target}; nothing to do")
            return
        if not SEMVER_RE.match(target):
            fail(f"invalid target version {target!r}")
        apply_version(current, target, args.dry_run)
        return

    if args.command == "check" and args.value:
        fail("unexpected extra argument after `check`; use --version instead")
    expected = read_expected_version(args.version)
    run_checks(expected)
    print(f"release metadata is consistent for {expected}")


if __name__ == "__main__":
    main()
