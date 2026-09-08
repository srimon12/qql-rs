.PHONY: all pipeline python node rust report clean

all: pipeline python node rust report

pipeline:
	.venv/bin/python pipeline/gen_corpus.py
	.venv/bin/python pipeline/embed.py

python:
	cd python && ../.venv/bin/python bench.py $(ARGS)

node:
	cd node && node bench.js $(ARGS)

rust:
	cd rust && ./target/release/vs-qdrant-bench $(ARGS)

report:
	.venv/bin/python common/count_loc.py
	.venv/bin/python common/render_results.py > results/tables.md

clean:
	rm -rf data/*.f32 data/*_bm25.json data/*_colbert* data/*.jsonl data/manifest.json results/*.json
