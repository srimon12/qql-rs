# qql-go Install Guide

Use this guide when the skill is installed on its own and the `qql-go` CLI is not available yet.

## Preferred install paths

Install the latest release on macOS or Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/srimon12/qql-go/main/install.sh | sh
```

Install on Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/srimon12/qql-go/main/install.ps1 | iex
```

Install with Go:

```bash
go install github.com/srimon12/qql-go/cmd/qql-go@latest
```

Build from source:

```bash
git clone https://github.com/srimon12/qql-go.git
cd qql-go
go build -o qql-go ./cmd/qql-go
```

## Expected binary name

The CLI binary is named:

- `qql-go` on macOS/Linux
- `qql-go.exe` on Windows

## PATH expectations

The helper script first checks:

1. `QQL_BIN`
2. `qql-go` on `PATH`
3. a repo-local fallback

If `qql-go` is installed somewhere custom, set:

```bash
export QQL_BIN=/absolute/path/to/qql-go
```

On Windows:

```powershell
$env:QQL_BIN = "C:\path\to\qql-go.exe"
```

## Local mode setup (self-hosted Qdrant + local embeddings)

`qql-go` supports three inference modes:

- `local` — local Qdrant + local embedding server (default)
- `cloud` — Qdrant Cloud inference
- `external` — any Qdrant + external embedding endpoint

For local mode, connect with the extra embedding flags:

### Windows (PowerShell)
```powershell
qql-go connect `
  --url http://localhost:6334 `
  --inference-mode local `
  --embedding-endpoint http://127.0.0.1:1234/v1/embeddings `
  --embedding-key <embedding-api-key> `
  --embedding-model text-embedding-all-minilm-l6-v2-embedding `
  --embedding-dimension 384
```

### Linux / macOS (Bash)
```bash
qql-go connect \
  --url http://localhost:6334 \
  --inference-mode local \
  --embedding-endpoint http://127.0.0.1:1234/v1/embeddings \
  --embedding-key <embedding-api-key> \
  --embedding-model text-embedding-all-minilm-l6-v2-embedding \
  --embedding-dimension 384
```

Requirements for local/external mode:

- `--embedding-endpoint` — an embedding endpoint API (e.g., standard /v1/embeddings used by Ollama, LM Studio, Cohere, etc.)
- `--embedding-key` — optional bearer token for hosted embedding providers
- `--embedding-model` — the model name to pass in the request
- `--embedding-dimension` — optional; auto-probed from the endpoint if omitted and reachable

For cloud mode, only `--url` (and `--secret` if needed) are required.
