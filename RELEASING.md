# QQL release procedure

All public packages use one repository version. The current release is `0.2.1`;
the corresponding Git tag is `v0.2.1`. The QQL language specification version
(`1.4`) is independent from the package release version.

## Published artifacts

| Registry | Packages |
|---|---|
| crates.io | `qql-core`, `qql-plan`, `qql-embed`, `qql`, `qql-edge`, `qql-cli` |
| PyPI | `pyqql`, `pyqql-edge` |
| npm | `@veristamp/nqql`, `@veristamp/nqql-edge`, `qql-wasm` |
| VS Code Marketplace | `srimon12.qql-lang` (extension version is independent; currently `0.2.3`) |
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
- `NPM_TOKEN` only for the first npm publication (optional after OIDC is wired).

### npm Trusted Publishing (required after first publish)

npm Trusted Publishing is **per package**. Configuring only the meta packages
is not enough — every platform package also needs its own Trusted Publisher.

For **each** of the packages listed below, on npmjs.com → Package → Settings →
Trusted Publisher, set **exactly**:

| Field | Value |
|---|---|
| Organization or user | `srimon12` |
| Repository | `qql-rs` |
| Workflow filename | `release.yml` (filename only, including extension) |
| Environment name | `release` |
| Allowed actions | `npm publish` |

Packages that must each have a Trusted Publisher:

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
qql-wasm
```

The first npm release still needs a short-lived granular access token because
npm can only attach Trusted Publishing after a package exists. Create a token
with package read/write access, permission to bypass 2FA for CI, and the
shortest practical expiration. Store it in the `release` GitHub environment for
that first publish only. After every package exists and Trusted Publishers are
configured, remove `NPM_TOKEN` — the release workflow authenticates via OIDC
(`id-token: write`) and does not use `NODE_AUTH_TOKEN`.

**CI requirements for OIDC** (enforced in `release.yml`):

- Node.js ≥ 22.14 (workflow uses Node 24 for publish jobs)
- npm CLI ≥ 11.5.1 (`npm install -g npm@latest` before publish)
- Job permission `id-token: write`
- GitHub-hosted runners only (self-hosted is not supported by npm OIDC)
- Do **not** point `actions/setup-node` at a token-backed `registry-url` for
  publish jobs; an empty `_authToken` in `.npmrc` blocks OIDC and yields
  `ENEEDAUTH`

### PyPI Trusted Publishing

Configure PyPI trusted publishers for both packages. The release workflow uses
separate GitHub environments:

| Package | GitHub environment |
|---|---|
| `pyqql` | `release-pyqql` |
| `pyqql-edge` | `release-pyqql-edge` |

For each pending/active publisher on PyPI:

- owner/repository: `srimon12/qql-rs`
- workflow: `release.yml`
- environment: match the table above

For projects that do not exist on PyPI yet, create pending trusted publishers
before pushing the first tag. PyPI creates each project on its first successful
OIDC publication, so no PyPI API token is required.

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
4. Refresh the bundled editor WASM so it matches `crates/qql-wasm`:

   ```bash
   wasm-pack build crates/qql-wasm --target nodejs --out-dir ../../editors/vscode/wasm
   ```

   The extension loads the bundle with plain `require()` in a CommonJS
   context, so the **`nodejs` target is required**. A `--target bundler`
   rebuild produces an ESM entry that imports `./qql_wasm_bg.wasm`, which
   Node 22 (CI) and the VS Code extension host cannot load
   (`ERR_UNKNOWN_FILE_EXTENSION`).

   The extension ships this copy; a stale bundle means diagnostics and
   formatting reject syntax that the grammar, snippets, and completions
   advertise. Verify the exports (e.g. `formatQuery`) exist in
   `editors/vscode/wasm/qql_wasm.d.ts` before continuing.
5. If the Qdrant protocol pin moves, re-sync the vendored API surfaces:

   ```bash
   python3 scripts/sync_qdrant_api.py --update --ref v1.19.0
   ```

   This refreshes `crates/qql-runtime/openapi.json` and the vendored protos
   from upstream Qdrant at an immutable commit, records the pin in
   `scripts/qdrant-api-manifest.json`, and is enforced by the CI
   `Qdrant API sync` job (`--check`).
6. If the extension changed, bump `editors/vscode/package.json`
   (its version is independent of the workspace version), run
   `npm run check` and `npm test` inside `editors/vscode/`, and publish with
   `npx vsce publish` — VSIX binaries are never committed.
6. Update release notes and user-facing installation documentation.
7. Validate synchronized metadata:

   ```bash
   python3 scripts/check_release.py --version 0.2.1
   ```

8. Open a pull request into `dev` and let CI pass.
9. Run the `Release` workflow manually from `dev`.

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
   python3 scripts/check_release.py --version 0.2.1
   ```

4. Create an annotated tag on that exact commit:

   ```bash
   git tag -a v0.2.1 -m "QQL 0.2.1"
   git push origin v0.2.1
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
cargo info --registry crates-io qql-core@0.2.1
cargo info --registry crates-io qql@0.2.1
cargo info --registry crates-io qql-edge@0.2.1
cargo install qql-cli@0.2.1 --locked --features edge

python -m pip install pyqql==0.2.1
python -m pip install pyqql-edge==0.2.1

npm view @veristamp/nqql@0.2.1
npm view @veristamp/nqql-edge@0.2.1
npm view qql-wasm@0.2.1
```

Install the CLI archive on at least one platform and verify
`qql version`, parsing, remote execution, and `qql --edge config edge` / `qql --edge doctor` (requires `--features edge`).

Registry releases are immutable. Never rebuild different bytes under an
already published version. If publication fails halfway through, inventory
every registry first, preserve the successful artifacts, and resume only the
missing packages with the exact same build outputs.
