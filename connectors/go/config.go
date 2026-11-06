package taba

import (
	"crypto/tls"
	"net/http"
	"time"
)

// Config is the client configuration (doc 21). It does not read environment variables: TABA_* is the prefix of
// the server configuration; whoever uses the connector reads their own and fills this struct. A zero field takes
// the default.
type Config struct {
	// BaseURL is the address of the client API load balancer (http://host:port or https://...). Required.
	BaseURL string
	// Token goes in Authorization: Bearer. Without it, the call goes without the header.
	Token string
	// ConnectTimeout is the time to open the connection (5 s).
	ConnectTimeout time.Duration
	// RequestTimeout is the total time of an ordinary call (30 s). consume uses WaitTime + 10 s.
	RequestTimeout time.Duration
	// MaxAttempts counts the first attempt (3).
	MaxAttempts int
	// UserAgent goes in User-Agent (taba-go/<version>). Add the application name.
	UserAgent string
	// TLSConfig is an own TLS configuration (a private CA, for example). Default: the system's roots, TLS 1.2
	// or later. When MinVersion is zero it is set to TLS 1.2.
	TLSConfig *tls.Config
	// HTTPClient replaces the connector's own HTTP client (ConnectTimeout and TLSConfig are then yours to set).
	HTTPClient *http.Client
}

const (
	defaultConnectTimeout = 5 * time.Second
	defaultRequestTimeout = 30 * time.Second
	defaultMaxAttempts    = 3
)

// Int returns a pointer to n, for the optional fields of the option structs.
func Int(n int) *int { return &n }

// Bool returns a pointer to b.
func Bool(b bool) *bool { return &b }

// String returns a pointer to s.
func String(s string) *string { return &s }

// NullableInt is a TopicUpdate field that can also be sent as null (back to the server default). Use SetInt or
// ClearInt; a nil *NullableInt leaves the field out of the request.
type NullableInt struct {
	value int
	null  bool
}

// SetInt sends n.
func SetInt(n int) *NullableInt { return &NullableInt{value: n} }

// ClearInt sends null: the field goes back to the server default.
func ClearInt() *NullableInt { return &NullableInt{null: true} }
