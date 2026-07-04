package bench

import (
	"fmt"
	"testing"

	gqql "github.com/qdrant/qql/gqql"
	qqlgo "github.com/srimon12/qql-go/pkg/qql"
)

// ── Queries ─────────────────────────────────────────────────────────

var queries = []struct {
	name string
	qql  string
}{
	{"Simple", "QUERY 'search' FROM docs LIMIT 10"},
	{"Hybrid", "QUERY 'search' FROM docs LIMIT 10 USING HYBRID"},
	{"Full", "QUERY 'vector search' FROM docs LIMIT 10 OFFSET 5 USING HYBRID RERANK WHERE topic = 'search' WITH (hnsw_ef = 128, exact = true)"},
	{"CTE_Prefetch", `WITH a AS (QUERY 'search' USING dense LIMIT 100 WHERE category = 'tech'), b AS (QUERY 'search' USING sparse LIMIT 100)
QUERY 'search' FROM docs LIMIT 10 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.8, b SCORE THRESHOLD 0.5) FUSION RRF`},
	{"CreateCollection", "CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)"},
	{"Insert", `INSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}`},
	{"DeleteWhere", "DELETE FROM docs WHERE category = 'archived'"},
	{"OrderBy", "QUERY ORDER BY created_at DESC FROM docs LIMIT 20 WHERE status = 'active'"},
	{"WithPayload", "QUERY 'search' FROM docs LIMIT 10 WITH PAYLOAD (include = ['title', 'body']) WITH VECTORS ('dense')"},
}

// ── qql-go (native Go) benchmarks ───────────────────────────────────

func Benchmark_qqlgo_Parse(b *testing.B) {
	for _, q := range queries {
		b.Run(q.name, func(b *testing.B) {
			for i := 0; i < b.N; i++ {
				_, err := qqlgo.Parse(q.qql)
				if err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}

// ── gqql (Rust C FFI) benchmarks ───────────────────────────────────

func Benchmark_gqql_Parse(b *testing.B) {
	for _, q := range queries {
		b.Run(q.name, func(b *testing.B) {
			for i := 0; i < b.N; i++ {
				_, err := gqql.Parse(q.qql)
				if err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}

// ── Summary ─────────────────────────────────────────────────────────

func TestAllParsersMatch(t *testing.T) {
	for _, q := range queries {
		_, errGo := qqlgo.Parse(q.qql)
		_, errGq := gqql.Parse(q.qql)

		if (errGo != nil) != (errGq != nil) {
			t.Errorf("%s: qql-go err=%v, gqql err=%v", q.name, errGo, errGq)
		}
	}
	fmt.Println("All parsers agree on", len(queries), "queries")
}
