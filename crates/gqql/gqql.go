package gqql

// #cgo LDFLAGS: -l:libgqql.a -lm
// #cgo CFLAGS: -I../../target/release
// #include <stdlib.h>
// extern char* qql_parse(const char* input);
// extern char* qql_tokenize(const char* input);
// extern void  qql_free_string(char* s);
import "C"
import (
	"errors"
	"strings"
	"unsafe"
)

const errPrefix = "gqql error: "

// Parse parses a QQL query string and returns the debug representation
// of the resulting AST, or an error if the query is invalid.
func Parse(input string) (string, error) {
	cInput := C.CString(input)
	defer C.free(unsafe.Pointer(cInput))

	cResult := C.qql_parse(cInput)
	if cResult == nil {
		return "", errors.New("gqql: null result")
	}
	defer C.qql_free_string(cResult)

	result := C.GoString(cResult)
	if strings.HasPrefix(result, errPrefix) {
		return "", errors.New(strings.TrimPrefix(result, errPrefix))
	}
	return result, nil
}

// Tokenize parses a QQL query string and returns a JSON array of tokens.
func Tokenize(input string) (string, error) {
	cInput := C.CString(input)
	defer C.free(unsafe.Pointer(cInput))

	cResult := C.qql_tokenize(cInput)
	if cResult == nil {
		return "", errors.New("gqql: null result")
	}
	defer C.qql_free_string(cResult)

	result := C.GoString(cResult)
	if strings.HasPrefix(result, errPrefix) {
		return "", errors.New(strings.TrimPrefix(result, errPrefix))
	}
	return result, nil
}
