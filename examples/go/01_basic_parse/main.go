package main

import (
	"fmt"
	"strings"

	"github.com/qdrant/qql/gqql"
)

func main() {
	// Parse a CREATE COLLECTION statement
	ast, _ := gqql.Parse("CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)")
	fmt.Println("=== Parsed AST ===")
	fmt.Println(ast[:min(len(ast), 500)])
	fmt.Println()

	// Tokenize a QUERY
	tokens, _ := gqql.Tokenize("QUERY 'vector database' FROM docs LIMIT 10")
	fmt.Println("=== Tokens ===")
	type tok struct{ Kind, Text string; Pos int }
	// parse JSON manually since Go SDK returns string
	tokens = strings.ReplaceAll(tokens, ",", "\n")
	fmt.Println(tokens[:min(len(tokens), 300)])
	fmt.Println()

	// Validate queries
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
