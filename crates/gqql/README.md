# gqql — Go bindings for QQL

Native Go SDK for the Qdrant Query Language (QQL) parser.

## Usage

```go
import "github.com/qdrant/qql/gqql"

func main() {
    result, err := gqql.Parse("CREATE COLLECTION docs")
    if err != nil {
        panic(err)
    }
    println(result)
}
```

## Build

```bash
make test
```

Requires Go and Rust toolchains.
