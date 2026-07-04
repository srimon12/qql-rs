module bench

go 1.25.5

require (
	github.com/qdrant/qql/gqql v0.0.0
	github.com/srimon12/qql-go v0.0.0
)

require (
	github.com/google/uuid v1.6.0 // indirect
	github.com/inconshreveable/mousetrap v1.1.0 // indirect
	github.com/qdrant/go-client v1.18.2 // indirect
	github.com/spf13/cobra v1.8.0 // indirect
	github.com/spf13/pflag v1.0.10 // indirect
	golang.org/x/net v0.53.0 // indirect
	golang.org/x/sys v0.43.0 // indirect
	golang.org/x/text v0.36.0 // indirect
	google.golang.org/genproto/googleapis/rpc v0.0.0-20260427160629-7cedc36a6bc4 // indirect
	google.golang.org/grpc v1.80.0 // indirect
	google.golang.org/protobuf v1.36.11 // indirect
)

replace github.com/qdrant/qql/gqql => ../crates/gqql

replace github.com/srimon12/qql-go => ../../qql-go
