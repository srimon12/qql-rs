package gqql

// #cgo LDFLAGS: -l:libgqql.a -lm
// #cgo CFLAGS: -I../../target/release
// #include <stdlib.h>
// extern char* qql_parse(const char* input);
// extern char* qql_tokenize(const char* input);
// extern char* qql_is_valid(const char* input);
// extern char* qql_inject_filter(const char* query, const char* field, const char* op, const char* value);
// extern void  qql_free_string(char* s);
import "C"
import (
	"errors"
	"strings"
	"unsafe"
)

const errPrefix = "gqql error: "

func decode(s string) (string, error) {
	if strings.HasPrefix(s, errPrefix) {
		return "", errors.New(strings.TrimPrefix(s, errPrefix))
	}
	return s, nil
}

func Parse(input string) (string, error) {
	cInput := C.CString(input)
	defer C.free(unsafe.Pointer(cInput))
	r := C.qql_parse(cInput)
	if r == nil {
		return "", errors.New("gqql: null result")
	}
	defer C.qql_free_string(r)
	return decode(C.GoString(r))
}

func Tokenize(input string) (string, error) {
	cInput := C.CString(input)
	defer C.free(unsafe.Pointer(cInput))
	r := C.qql_tokenize(cInput)
	if r == nil {
		return "", errors.New("gqql: null result")
	}
	defer C.qql_free_string(r)
	return decode(C.GoString(r))
}

func IsValid(input string) bool {
	cInput := C.CString(input)
	defer C.free(unsafe.Pointer(cInput))
	return C.GoString(C.qql_is_valid(cInput)) == "true"
}

func InjectFilter(query, field, op, value string) (string, error) {
	cQ := C.CString(query)
	cF := C.CString(field)
	cO := C.CString(op)
	cV := C.CString(value)
	defer C.free(unsafe.Pointer(cQ))
	defer C.free(unsafe.Pointer(cF))
	defer C.free(unsafe.Pointer(cO))
	defer C.free(unsafe.Pointer(cV))
	r := C.qql_inject_filter(cQ, cF, cO, cV)
	if r == nil {
		return "", errors.New("gqql: null result")
	}
	defer C.qql_free_string(r)
	return decode(C.GoString(r))
}
