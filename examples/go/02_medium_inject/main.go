package main

import (
	"fmt"

	"github.com/qdrant/qql/gqql"
)

func main() {
	q := "QUERY 'machine learning transformer' FROM papers LIMIT 20"

	r, _ := gqql.InjectFilter(q, "tenant_id", "=", `{"str": "acme-corp"}`)
	fmt.Println("=== String filter ===")
	fmt.Println(trunc(r, 400))

	r, _ = gqql.InjectFilter(q, "impact_factor", ">=", `{"float": 5.0}`)
	fmt.Println("\n=== Numeric filter ===")
	fmt.Println(trunc(r, 400))

	r, _ = gqql.InjectFilter(q, "is_published", "=", `{"bool": true}`)
	fmt.Println("\n=== Boolean filter ===")
	fmt.Println(trunc(r, 400))
}

func trunc(s string, n int) string {
	if len(s) > n {
		return s[:n]
	}
	return s
}
