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

func TestParseDeleteById(t *testing.T) {
	r, err := Parse("DELETE FROM docs WHERE id = 42")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(r, "Delete") {
		t.Errorf("expected Delete in result, got: %s", r)
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
