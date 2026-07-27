# QQL release procedure

All public packages use one repository version. The current release is `0.1.2`;
the corresponding Git tag is `v0.1.2`. The QQL language specification version
(`1.1`) is independent from the package release version.

## Published artifacts

| Registry | Packages |
|---|---|
| crates.io | `qql-core`, `qql-plan`, `qql-embed`, `qql`, `qql-edge`, `qql-cli` |
| PyPI | `pyqql`, `pyqql-edge` |
| npm | `@veristamp/nqql`, `@veristamp/nqql-edge`, `@veristamp/qql-wasm` |
| GitHub Releases | Default REST/gRPC `qql` CLI archives and checksums |

`qql-conformance`, `qql-grammar-gen`, and the Rust implementation crates for
Python, Node.js, and WASM set `publish = false` and must never be sent to
crates.io.

The native Node.js packages use platform packages. The first supported targets
are:

- Linux x86-64 with glibc;
- macOS x86-64;
- macOS Apple Silicon;
- Windows x86-64.

FastEmbed model weights are downloaded on first use. They are not included in
Python wheels, npm packages, or CLI archives.

## One-time registry setup

Create a GitHub Actions environment named `release`, restrict its deployment
policy to tags matching `v*`, and add:

- `CRATES_IO_TOKEN` for the crates.io publishing account;
- `NPM_TOKEN` for the first npm publication.

The first npm release needs a short-lived granular access token because npm
trusted publishing can only be attached after a package exists. Create a token
with package read/write access, permission to bypass 2FA for CI, and the
shortest practical expiration. Store it in the `release` GitHub environment.
After the first successful npm release creates every root and platform package,
configure npm trusted publishing for `srimon12/qql-rs`, workflow `release.yml`,
environment `release`, then remove `NPM_TOKEN`.

Configure PyPI trusted publishers for both `pyqql` and `pyqql-edge`:

- owner/repository: `srimon12/qql-rs`;
- workflow: `release.yml`;
- environment: `release`.

For projects that do not exist on PyPI yet, create pending trusted publishers
before pushing the first tag. Create one pending publisher for `pyqql` and a
second for `pyqql-edge`; PyPI will create each project on its first successful
OIDC publication, so no PyPI API token is required.

The npm account must own the root package names and every generated platform
package name:

```text
@veristamp/nqql
@veristamp/nqql-linux-x64-gnu
@veristamp/nqql-darwin-x64
@veristamp/nqql-darwin-arm64
@veristamp/nqql-win32-x64-msvc
@veristamp/nqql-edge
@veristamp/nqql-edge-linux-x64-gnu
@veristamp/nqql-edge-darwin-arm64
@veristamp/nqql-edge-win32-x64-msvc
@veristamp/qql-wasm
```

Do not create the first tag until all package names and publishing identities
have been verified.

## Branch protections

Configure repository rules for `main`:

- require a pull request before merging;
- require the CI checks;
- block force pushes and branch deletion;
- apply the rules to administrators;
- do not allow direct pushes.

Protect `dev` with required CI checks and pull requests as well. The checked-in
CI workflow additionally rejects a pull request into `main` unless its source
branch is exactly `dev`.

Workflow files cannot enforce repository permissions by themselves. The
server-side branch rules are therefore mandatory.

## Prepare a release

1. Work on a topic branch created from `dev`.
2. Update the single workspace version in `Cargo.toml`.
3. Update the matching versions in:
   - `crates/pyqql/pyproject.toml`;
   - `crates/pyqql-edge/pyproject.toml`;
   - `crates/nqql/package.json`;
   - `crates/nqql-edge/package.json`;
   - Node optional platform dependencies.
4. Update release notes and user-facing installation documentation.
5. Validate synchronized metadata:

   ```bash
   python3 scripts/check_release.py --version 0.1.1
   ```

6. Open a pull request into `dev` and let CI pass.
7. Run the `Release` workflow manually from `dev`.

A manual Release run builds and packages every artifact but has no publishing
jobs. Download and inspect:

- six `.crate` archives;
- CLI archives for each supported target;
- both Python wheels for each supported target;
- root and platform npm tarballs;
- the `qql-wasm` npm tarball.

Install the artifacts in clean temporary projects before approving the release.

## Publish

1. Open the release pull request from `dev` into `main`.
2. Merge it after every required check passes.
3. Update the local `main` branch and confirm its commit:

   ```bash
   git switch main
   git pull --ff-only origin main
   python3 scripts/check_release.py --version 0.1.1
   ```

4. Create an annotated tag on that exact commit:

   ```bash
   git tag -a v0.1.1 -m "QQL 0.1.1"
   git push origin v0.1.1
   ```

Only the tag push can publish. The release gate verifies that:

- the tag version matches every package;
- the tagged commit is already contained in `origin/main`;
- grammar generation and language conformance pass;
- formatting, Clippy, tests, and packaging pass;
- all platform artifacts finish building before any registry job begins.

crates.io publication follows dependency order:

```text
qql-core
├── qql-plan
└── qql-embed
    └── qql
        └── qql-edge
            └── qql-cli
```

The workflow waits for each newly published dependency to appear in the
crates.io index before publishing its dependents. Native npm platform packages
are published before their root dispatcher packages.

## Verify the public release

After the workflow succeeds:

```bash
cargo info --registry crates-io qql-core@0.1.1
cargo info --registry crates-io qql@0.1.1
cargo info --registry crates-io qql-edge@0.1.1
cargo install qql-cli@0.1.1 --locked

python -m pip install pyqql==0.1.1
python -m pip install pyqql-edge==0.1.1

npm view @veristamp/nqql@0.1.1
npm view @veristamp/nqql-edge@0.1.1
npm view @veristamp/qql-wasm@0.1.1
```

Install the CLI archive on at least one platform and verify
`qql version`, parsing, remote execution, and `qql --edge doctor`.

Registry releases are immutable. Never rebuild different bytes under an
already published version. If publication fails halfway through, inventory
every registry first, preserve the successful artifacts, and resume only the
missing packages with the exact same build outputs.
