#!/usr/bin/env python3
"""Validate the synchronized QQL release metadata before packaging."""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
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
PYTHON_PACKAGES = ("pyqql", "pyqql-edge")
NODE_PACKAGES = ("nqql", "nqql-edge")


def load_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def fail(message: str) -> None:
    print(f"release metadata error: {message}", file=sys.stderr)
    raise SystemExit(1)


def package_version(package: dict, workspace_version: str) -> str:
    value = package.get("version")
    if isinstance(value, dict) and value.get("workspace") is True:
        return workspace_version
    if isinstance(value, str):
        return value
    fail("Cargo package does not declare a version")


def validate_cargo(expected: str) -> None:
    root = load_toml(ROOT / "Cargo.toml")
    workspace_package = root["workspace"]["package"]
    workspace_version = workspace_package["version"]
    if workspace_version != expected:
        fail(
            f"workspace version is {workspace_version}, expected release {expected}"
        )
    for key in ("authors", "license", "repository", "homepage", "rust-version"):
        if not workspace_package.get(key):
            fail(f"workspace.package.{key} is missing")

    for crate in PUBLIC_CRATES + PRIVATE_CRATES:
        manifest = ROOT / "crates" / crate / "Cargo.toml"
        data = load_toml(manifest)
        package = data["package"]
        actual = package_version(package, workspace_version)
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


def validate_editor() -> None:
    """The VS Code extension version is intentionally independent from the
    workspace version (packaging slot), but it must exist and its bundled WASM
    copy must declare the same version."""
    ext = json.loads((ROOT / "editors" / "vscode" / "package.json").read_text())
    if not isinstance(ext.get("version"), str) or not ext["version"]:
        fail("editors/vscode/package.json is missing a version")
    bundled = ROOT / "editors" / "vscode" / "wasm" / "package.json"
    if not bundled.is_file():
        fail("editors/vscode/wasm/package.json is missing (bundled editor WASM)")
    wasm_pkg = json.loads(bundled.read_text())
    if wasm_pkg.get("name") != "qql-wasm":
        fail("editors/vscode/wasm/package.json must be the qql-wasm bundle")
    if not wasm_pkg.get("version"):
        fail("editors/vscode/wasm/package.json is missing a version")
    for export in ("formatQuery",):
        d_ts = ROOT / "editors" / "vscode" / "wasm" / "qql_wasm.d.ts"
        if not d_ts.is_file():
            fail("editors/vscode/wasm/qql_wasm.d.ts is missing (stale bundle?)")
        if export not in d_ts.read_text():
            fail(
                f"bundled editor WASM does not export {export}; "
                "rebuild with wasm-pack from crates/qql-wasm"
            )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--version",
        help="Expected release version. Defaults to the VERSION file at the repo root.",
    )
    args = parser.parse_args()

    if args.version:
        expected = args.version
    else:
        version_file = ROOT / "VERSION"
        if version_file.is_file():
            expected = version_file.read_text().strip()
        else:
            root_toml = load_toml(ROOT / "Cargo.toml")
            expected = root_toml["workspace"]["package"]["version"]
    if expected.startswith("v"):
        expected = expected[1:]

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

    validate_cargo(expected)
    validate_python(expected)
    validate_node(expected)
    validate_editor()
    print(f"release metadata is consistent for {expected}")


if __name__ == "__main__":
    main()
