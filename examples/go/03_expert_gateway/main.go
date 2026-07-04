package main

import (
	"fmt"

	"github.com/qdrant/qql/gqql"
)

type userCtx struct {
	Tenant string
	Role   string
}

var users = map[string]userCtx{
	"alice":   {"acme", "admin"},
	"bob":     {"acme", "viewer"},
	"charlie": {"globex", "viewer"},
}

func enforce(user, query string) (string, error) {
	ctx, ok := users[user]
	if !ok {
		return "", fmt.Errorf("unknown user: %s", user)
	}
	if !gqql.IsValid(query) {
		return "", fmt.Errorf("invalid QQL query")
	}
	return gqql.InjectFilter(query, "tenant_id", "=", fmt.Sprintf(`{"str": "%s"}`, ctx.Tenant))
}

func main() {
	requests := []struct{ user, query string }{
		{"alice", "QUERY 'sales data' FROM analytics LIMIT 10"},
		{"bob", "QUERY 'sales data' FROM analytics LIMIT 10"},
		{"charlie", "QUERY 'engineering docs' FROM docs LIMIT 5"},
	}

	fmt.Println("=== QQL Query Gateway ===")
	for _, r := range requests {
		safe, err := enforce(r.user, r.query)
		if err != nil {
			fmt.Printf("  ERROR: %v\n", err)
			continue
		}
		fmt.Printf("\n  user=%-8s role=%-7s\n", r.user, users[r.user].Role)
		fmt.Printf("  raw:  %s\n", r.query)
		fmt.Printf("  safe: %s\n", trunc(safe, 130))
	}
	fmt.Println("\n  → QQL inject_filter enables auth middleware that rewrites")
	fmt.Println("    queries before they reach Qdrant — no per-tenant collections.")
}

func trunc(s string, n int) string {
	if len(s) > n {
		return s[:n] + "..."
	}
	return s
}
