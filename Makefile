ROOT := $(realpath .)
GO_LDFLAGS := CGO_LDFLAGS="-L$(ROOT)/target/release -l:libgqql.a -lm"
PYO3 := PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1

.PHONY: help build test bench examples clean
.PHONY: build-rust build-go build-py build-node build-wasm
.PHONY: test-rust test-go test-py test-node
.PHONY: bench-rust bench-go bench-py bench-node bench-all
.PHONY: examples-rust examples-go examples-py examples-node examples-all
.PHONY: sdk-link sdk-go sdk-py sdk-node sdk-wasm

# ── Default ──────────────────────────────────────────────────────────

help:
	@echo "QQL Makefile"
	@echo ""
	@echo "Build targets:"
	@echo "  make build          Build all Rust crates (release)"
	@echo "  make build-rust     Build Rust crates only"
	@echo "  make build-go       Build gqql C FFI + Go SDK"
	@echo "  make build-py       Build pyqql Python binding"
	@echo "  make build-node     Build nqql Node.js binding"
	@echo "  make build-wasm     Build qql-wasm WASM binding"
	@echo ""
	@echo "Test targets:"
	@echo "  make test           Run all tests (Rust + Go)"
	@echo "  make test-rust      Run Rust tests (qql-core + qql)"
	@echo "  make test-go        Run Go gqql tests"
	@echo ""
	@echo "Benchmark targets:"
	@echo "  make bench          Run all parser benchmarks"
	@echo "  make bench-rust     Rust qql-core benchmark"
	@echo "  make bench-go       Go (qql-go + gqql) benchmarks"
	@echo "  make bench-py       Python pyqql benchmark"
	@echo "  make bench-node     Node.js nqql benchmark"
	@echo ""
	@echo "Example targets:"
	@echo "  make examples       Run all examples"
	@echo "  make examples-rust  Run all Rust examples"
	@echo "  make examples-go    Run all Go examples"
	@echo "  make examples-py    Check all Python examples"
	@echo "  make examples-node  Check all Node.js examples"
	@echo ""
	@echo "SDK helpers:"
	@echo "  make sdk-link       Create .so symlinks for Python/Node.js"
	@echo ""
	@echo "Utilities:"
	@echo "  make clean          Clean all build artifacts"
	@echo "  make help           Show this help"

# ── Build ────────────────────────────────────────────────────────────

build: build-rust build-go

build-rust:
	cargo build --release

build-go: build-rust
	cargo build --release -p gqql

build-py:
	$(PYO3) cargo build --release -p pyqql

build-node:
	cargo build --release -p nqql

build-wasm:
	cargo build --release -p qql-wasm

# ── SDK symlinks ─────────────────────────────────────────────────────

sdk-link:
	ln -sf libpyqql.so $(ROOT)/target/release/pyqql.so
	cp $(ROOT)/target/release/libnqql.so $(ROOT)/target/release/nqql.node

# ── Test ─────────────────────────────────────────────────────────────

test: test-rust test-go

test-rust:
	cargo test -p qql-core -p qql

test-go: build-go
	cd $(ROOT)/crates/gqql && \
	$(GO_LDFLAGS) CGO_ENABLED=1 go test -v ./...

# ── Benchmarks ───────────────────────────────────────────────────────

bench: bench-rust bench-go bench-py bench-node

bench-rust: build-rust
	cargo run --release --manifest-path bench/bench_rust/Cargo.toml

bench-go: build-go
	cd $(ROOT)/bench && $(GO_LDFLAGS) CGO_ENABLED=1 go test -bench=. -benchmem -count=3 2>&1 | grep -E "^(Benchmark|---)"

bench-py: build-py sdk-link
	PYTHONPATH=$(ROOT)/target/release python3 bench/bench_python.py

bench-node: build-node sdk-link
	node bench/bench_node.js

# ── Examples ─────────────────────────────────────────────────────────

examples: examples-rust examples-go examples-py examples-node

examples-rust:
	@for f in examples/rust/*/Cargo.toml; do \
	  echo "\n=== $$(basename $$(dirname $$f)) ==="; \
	  cargo run --release --manifest-path "$$f"; \
	done

examples-go: build-go
	@for d in examples/go/*/; do \
	  echo "\n=== $$(basename $$d) ==="; \
	  cd "$$d" && $(GO_LDFLAGS) CGO_ENABLED=1 go run .; \
	  cd $(ROOT); \
	done

examples-py: build-py sdk-link
	@for f in examples/python/*.py; do \
	  echo "\n=== $$(basename $$f) ==="; \
	  PYTHONPATH=$(ROOT)/target/release python3 "$$f"; \
	done

examples-node: build-node sdk-link
	@for f in examples/nodejs/*.mjs; do \
	  echo "\n=== $$(basename $$f) ==="; \
	  node "$$f"; \
	done

# ── Clean ────────────────────────────────────────────────────────────

clean:
	cargo clean
	rm -f $(ROOT)/target/release/pyqql.so
	rm -f $(ROOT)/target/release/nqql.node
