package main

import (
	"fmt"

	"github.com/qdrant/qql/gqql"
)

func main() {
	ast, _ := gqql.Parse("CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)")
	fmt.Println("=== Parsed AST ===")
	fmt.Println(trunc(ast, 500))

	fmt.Println("\n=== Tokens ===")
	tokens, _ := gqql.Tokenize("QUERY 'vector database' FROM docs LIMIT 10")
	fmt.Println(tokens[:truncIdx(tokens, 400)])

	fmt.Println("\n=== Validation ===")
	for _, q := range []string{
		"QUERY 'hello' FROM docs LIMIT 5",
		"CREATE COLLECTION docs",
		"SELECT * FROM docs WHERE id = 1",
		"",
		"BOGUS STUFF",
	} {
		fmt.Printf("  valid=%-5t  %q\n", gqql.IsValid(q), q)
	}
}

func trunc(s string, n int) string {
	if len(s) > n {
		return s[:n]
	}
	return s
}

func truncIdx(s string, n int) int {
	if len(s) > n {
		return n
	}
	return len(s)
}
