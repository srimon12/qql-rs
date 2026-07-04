package main

import (
	"fmt"

	"github.com/qdrant/qql/gqql"
)

func main() {
	userQuery := "QUERY 'machine learning transformer' FROM papers LIMIT 20"
	fmt.Printf("User query valid: %t\n", gqql.IsValid(userQuery))

	// Inject a tenant_id filter (string value)
	tenantQuery, _ := gqql.InjectFilter(
		userQuery, "tenant_id", "=", `{"str": "acme-corp"}`,
	)
	fmt.Println("\n=== Tenant isolation ===")
	fmt.Println(trunc(tenantQuery, 500))

	// Inject a numeric threshold
	boosted, _ := gqql.InjectFilter(
		userQuery, "impact_factor", ">=", `{"float": 5.0}`,
	)
	fmt.Println("\n=== Numeric threshold ===")
	fmt.Println(trunc(boosted, 500))

	// Inject a boolean flag
	published, _ := gqql.InjectFilter(
		userQuery, "is_published", "=", `{"bool": true}`,
	)
	fmt.Println("\n=== Boolean filter ===")
	fmt.Println(trunc(published, 500))
}

func trunc(s string, n int) string {
	if len(s) > n {
		return s[:n] + "..."
	}
	return s
}
