package main

import (
	"fmt"

	"github.com/qdrant/qql/gqql"
)

var users = map[string]struct{ Tenant, Role string }{
	"alice":   {"acme", "admin"},
	"bob":     {"acme", "viewer"},
	"charlie": {"globex", "viewer"},
}

func enforce(user, query string) (string, error) {
	ctx := users[user]
	safe, err := gqql.InjectFilter(query, "tenant_id", "=", fmt.Sprintf(`{"str": "%s"}`, ctx.Tenant))
	if err != nil {
		return "", err
	}
	if ctx.Role == "viewer" {
		safe, err = gqql.InjectFilter(safe, "status", "!=", `{"str": "confidential"}`)
		if err != nil {
			return "", err
		}
	}
	return safe, nil
}

func main() {
	requests := []struct{ user, query string }{
		{"alice", "QUERY 'sales data' FROM analytics LIMIT 10"},
		{"bob", "QUERY 'sales data' FROM analytics LIMIT 10"},
		{"charlie", "QUERY 'engineering docs' FROM docs LIMIT 5"},
	}

	fmt.Println("=== QQL Query Gateway ===")
	for _, r := range requests {
		safe, _ := enforce(r.user, r.query)
		fmt.Printf("\n  user=%-8s role=%-7s\n", r.user, users[r.user].Role)
		fmt.Printf("  raw:  %s\n", r.query)
		fmt.Printf("  safe: %s\n", trunc(safe, 130))
	}
}

func trunc(s string, n int) string {
	if len(s) > n {
		return s[:n]
	}
	return s
}
