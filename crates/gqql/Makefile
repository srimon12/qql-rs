ROOT := $(realpath ../../)

.PHONY: build test clean

build:
	cd $(ROOT) && cargo build --release -p gqql

test: build
	cd $(ROOT)/crates/gqql && \
	CGO_LDFLAGS="-L$(ROOT)/target/release -l:libgqql.a -lm" \
	CGO_ENABLED=1 go test -v ./...

clean:
	cd $(ROOT) && cargo clean -p gqql
