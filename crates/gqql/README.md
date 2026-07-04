# gqql — Go bindings for QQL

Native Go SDK for the Qdrant Query Language (QQL) parser, compiled via CGO and Rust FFI.

## Features

- **Native parsing**: Rust-speed QQL parsing in Go
- **Validation**: Check if a query string is valid QQL
- **Filter injection**: Add tenant isolation filters to parsed queries

## Usage

```go
import "github.com/srimon12/qql-rs/crates/gqql"

func main() {
    // Parse to JSON string
    result, err := gqql.Parse("CREATE COLLECTION docs")
    if err != nil {
        panic(err)
    }
    println(result)

    // Validate
    valid := gqql.IsValid("QUERY 'search' FROM docs LIMIT 10")
    println(valid)

    // Inject filter
    secured, err := gqql.InjectFilter(
        "QUERY 'patients' FROM medical LIMIT 5",
        "org_id", "=", "acme-corp",
    )
}
```

## API

| Function | Returns | Description |
|---|---|---|
| `Parse(input)` | `(string, error)` | Parse single statement → JSON |
| `ParseAll(input)` | `(string, error)` | Parse multiple statements → JSON array |
| `ParseBatch(queries)` | `(string, error)` | Parse array of query strings |
| `IsValid(input)` | `bool` | Check if query string is valid QQL |
| `InjectFilter(query, field, op, value)` | `(string, error)` | Inject filter into query AST |
| `Tokenize(input)` | `string` | Tokenize query string |

## Build

Requires Go toolchain and Rust toolchain:

```bash
make test
```
