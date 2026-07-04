package gqql

import (
	"strings"
	"testing"
)

func TestParseCreateCollection(t *testing.T) {
	r, err := Parse("CREATE COLLECTION docs")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(r, "CreateCollection") {
		t.Errorf("expected CreateCollection in result, got: %s", r)
	}
}

func TestParseInsert(t *testing.T) {
	r, err := Parse(`INSERT INTO docs VALUES {id: 1, text: "hello"}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(r, "Insert") {
		t.Errorf("expected Insert in result, got: %s", r)
	}
}

func TestParseQuery(t *testing.T) {
	r, err := Parse("QUERY 'test' FROM docs LIMIT 10")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(r, "Query") {
		t.Errorf("expected Query in result, got: %s", r)
	}
}

func TestParseSyntaxError(t *testing.T) {
	_, err := Parse("CREATE INVALID")
	if err == nil {
		t.Fatal("expected error")
	}
}

func TestParseEmptyInput(t *testing.T) {
	_, err := Parse("")
	if err == nil {
		t.Fatal("expected error")
	}
}

func TestTokenize(t *testing.T) {
	r, err := Tokenize("CREATE COLLECTION docs")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(r, "CREATE") {
		t.Errorf("expected CREATE in tokens, got: %s", r)
	}
}

func TestTokenizeInvalid(t *testing.T) {
	_, err := Tokenize("!")
	if err == nil {
		t.Fatal("expected error")
	}
}

func TestIsValid(t *testing.T) {
	if !IsValid("CREATE COLLECTION docs") {
		t.Error("expected true")
	}
	if IsValid("CREATE INVALID") {
		t.Error("expected false")
	}
	if IsValid("") {
		t.Error("expected false for empty")
	}
}

func TestInjectFilterOnQuery(t *testing.T) {
	r, err := InjectFilter(`QUERY 'hello' FROM docs LIMIT 10`, "tenant_id", "=", `{"str": "acme"}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(r, "tenant_id") {
		t.Errorf("expected tenant_id in result, got: %s", r)
	}
	if !strings.Contains(r, "acme") {
		t.Errorf("expected acme in result, got: %s", r)
	}
}

func TestInjectFilterOnDelete(t *testing.T) {
	r, err := InjectFilter(`DELETE FROM docs WHERE id = 1`, "tenant_id", "=", `{"str": "acme"}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(r, "tenant_id") {
		t.Errorf("expected tenant_id in result, got: %s", r)
	}
}

func TestInjectFilterWithIntValue(t *testing.T) {
	r, err := InjectFilter(`QUERY 'test' FROM docs LIMIT 5`, "score", ">", `{"float": 0.5}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(r, "score") {
		t.Errorf("expected score in result, got: %s", r)
	}
}
