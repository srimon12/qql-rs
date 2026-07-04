package main

import (
	"fmt"

	"github.com/qdrant/qql/gqql"
)

func main() {
	script := `
CREATE COLLECTION docs HYBRID;
INSERT INTO docs VALUES {id: 1, text: "first"};
INSERT INTO docs VALUES {id: 2, text: "second"};
QUERY "test" FROM docs LIMIT 10;
`
	stmts, err := gqql.ParseAll(script)
	if err != nil {
		panic(err)
	}
	fmt.Println("=== Script Parsing (ParseAll) ===")
	fmt.Printf("Parsed %d statements from a .qql script:\n", len(stmts))
	for i, s := range stmts {
		fmt.Printf("  [%d] %s...\n", i, trunc(s, 80))
	}

	queries := []string{
		"QUERY 'alpha' FROM docs LIMIT 5",
		"QUERY 'beta'  FROM docs LIMIT 5",
		"QUERY 'gamma' FROM docs LIMIT 5",
	}
	results, err := gqql.ParseBatch(queries)
	if err != nil {
		panic(err)
	}
	fmt.Println("\n=== Batch Parsing (ParseBatch) ===")
	fmt.Printf("Parsed %d queries in a single FFI call:\n", len(results))
	for i, r := range results {
		fmt.Printf("  [%d] %s...\n", i, trunc(r, 80))
	}
}

func trunc(s string, n int) string {
	if len(s) > n {
		return s[:n]
	}
	return s
}
