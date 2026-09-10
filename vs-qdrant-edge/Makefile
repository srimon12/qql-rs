# QQL Edge vs Qdrant Edge head-to-head.
#
# Reuses the vs-qdrant virtualenv (numpy + fastembed + qdrant-edge-py).
PY := ../vs-qdrant/.venv/bin/python

bench:
	$(PY) bench.py

quick:
	$(PY) bench.py --reps 1 --iters 5 --skip-colbert

.PHONY: bench quick
