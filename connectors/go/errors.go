package taba

import "fmt"

// Version is the version of the connector; ContractVersion, the one of the common contract it implements.
const (
	Version         = "0.1.0"
	ContractVersion = "1"
)

// Error is the only kind of error of the connector (doc 21, "The common contract"): the API ErrorResponse, or a
// failure of the client itself (Status 0 and a Code client.network, client.timeout, client.invalid-config,
// client.invalid-response or client.invalid-request). Test the Code, never the Message.
//
// A cancelled context is not an *Error: the call returns the context's own error (context.Canceled).
type Error struct {
	// Status is the HTTP status, or zero on a failure of the client itself.
	Status int
	// Code is the group.reason code of the error.
	Code    string
	Message string
	// RequestID is the requestId of the ErrorResponse, or the X-Request-Id header.
	RequestID string
	// RetryAfter is the seconds of the Retry-After header; zero when the response did not carry it (see HasRetryAfter).
	RetryAfter int
	// Details is the details field of the ErrorResponse, as JSON text.
	Details string
	// Body is the response body as it came (empty on a client failure).
	Body []byte

	hasRetryAfter bool
}

// HasRetryAfter tells whether the response carried a Retry-After header.
func (e *Error) HasRetryAfter() bool { return e.hasRetryAfter }

// IsClientSide is true for a failure of the client itself, not a server response.
func (e *Error) IsClientSide() bool { return e.Status == 0 }

// Error implements the error interface.
func (e *Error) Error() string {
	if e.RequestID != "" {
		return fmt.Sprintf("%s: %s (requestId %s)", e.Code, e.Message, e.RequestID)
	}
	return fmt.Sprintf("%s: %s", e.Code, e.Message)
}

func clientError(code, format string, args ...interface{}) *Error {
	return &Error{Code: code, Message: fmt.Sprintf(format, args...)}
}

// lostOrGone: the message already went back to the queue (deadline expired) or is already gone.
func lostOrGone(err error) bool {
	e, ok := err.(*Error)
	return ok && (e.Status == 409 || e.Status == 404)
}
