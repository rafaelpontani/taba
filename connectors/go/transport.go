package taba

import (
	"bytes"
	"context"
	"crypto/tls"
	"io/ioutil"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

// apiCall is the description of a request to the API: method, path, query, body, headers and the rules of the
// call (timeout, retry of 502/504, responses that count as success after a 503).
type apiCall struct {
	method string
	// path has the segments already encoded (/topics/orders).
	path        string
	query       [][2]string
	body        []byte
	hasBody     bool
	contentType string
	requestID   string
	traceparent string
	// timeout overrides the total time of the call (consume uses wait_time + 10).
	timeout time.Duration
	// retryGateway retries 502 and 504: only publishing to a FIFO queue, where the primary deduplicates.
	retryGateway bool
	// recover: after a 503, these responses count as success (the operation had already been applied).
	recover []recoverRule
}

type recoverRule struct {
	status int
	code   string
}

func (c *apiCall) withQuery(key, value string) *apiCall {
	c.query = append(c.query, [2]string{key, value})
	return c
}

func (c *apiCall) withJSON(text []byte) *apiCall {
	c.body, c.hasBody = text, true
	return c
}

type apiReply struct {
	status    int
	body      []byte
	recovered bool
}

// segment percent-encodes a path segment or a query component (a space is %20, never +).
func segment(value string) string {
	return strings.Replace(url.QueryEscape(value), "+", "%20", -1)
}

type sleepFunc func(ctx context.Context, d time.Duration) error

func defaultSleep(ctx context.Context, d time.Duration) error {
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-timer.C:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// retryable is a failure before the body was sent (refused, DNS): the call can be repeated like a 503.
type retryable struct{ err *Error }

func (r *retryable) Error() string { return r.err.Error() }

type transport struct {
	config   Config
	base     string
	prefix   string
	client   *http.Client
	userAgnt string
	sleep    sleepFunc
	owned    *http.Transport
}

func newTransport(config Config) (*transport, error) {
	base := strings.TrimSpace(config.BaseURL)
	u, err := url.Parse(base)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" {
		shown := base
		if shown == "" {
			shown = "empty"
		}
		return nil, clientError("client.invalid-config", "BaseURL must be an http:// or https:// address (received: %s)", shown)
	}
	if config.ConnectTimeout <= 0 {
		config.ConnectTimeout = defaultConnectTimeout
	}
	if config.RequestTimeout <= 0 {
		config.RequestTimeout = defaultRequestTimeout
	}
	if config.MaxAttempts <= 0 {
		config.MaxAttempts = defaultMaxAttempts
	}
	if config.UserAgent == "" {
		config.UserAgent = "taba-go/" + Version
	}
	t := &transport{
		config: config,
		base:   u.Scheme + "://" + u.Host,
		prefix: strings.TrimRight(u.Path, "/"),
		sleep:  defaultSleep,
	}
	if config.HTTPClient != nil {
		t.client = config.HTTPClient
		return t, nil
	}
	tlsConfig := config.TLSConfig
	if tlsConfig == nil {
		tlsConfig = &tls.Config{}
	}
	if tlsConfig.MinVersion == 0 {
		tlsConfig.MinVersion = tls.VersionTLS12
	}
	dialer := &net.Dialer{Timeout: config.ConnectTimeout, KeepAlive: 30 * time.Second}
	t.owned = &http.Transport{
		Proxy:               http.ProxyFromEnvironment,
		DialContext:         dialer.DialContext,
		TLSClientConfig:     tlsConfig,
		MaxIdleConnsPerHost: 16,
		IdleConnTimeout:     90 * time.Second,
	}
	t.client = &http.Client{Transport: t.owned}
	return t, nil
}

func (t *transport) close() {
	if t.owned != nil {
		t.owned.CloseIdleConnections()
	}
}

func (t *transport) url(call *apiCall) string {
	u := t.base + t.prefix + call.path
	if len(call.query) > 0 {
		parts := make([]string, len(call.query))
		for i, kv := range call.query {
			parts[i] = segment(kv[0]) + "=" + segment(kv[1])
		}
		u += "?" + strings.Join(parts, "&")
	}
	return u
}

func isDialError(err error) (dial bool, timeout bool) {
	if u, ok := err.(*url.Error); ok {
		err = u.Err
	}
	op, ok := err.(*net.OpError)
	if !ok || op.Op != "dial" {
		return false, false
	}
	n, isNet := op.Err.(net.Error)
	return true, isNet && n.Timeout()
}

func isTimeout(err error) bool {
	if u, ok := err.(*url.Error); ok {
		err = u.Err
	}
	n, ok := err.(net.Error)
	return ok && n.Timeout()
}

// once makes one attempt. A cancelled context comes back as the context's error.
func (t *transport) once(ctx context.Context, call *apiCall, timeout time.Duration) (int, http.Header, []byte, error) {
	reqCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	var body *bytes.Reader
	var req *http.Request
	var err error
	if call.hasBody {
		body = bytes.NewReader(call.body)
		req, err = http.NewRequest(call.method, t.url(call), body)
	} else {
		req, err = http.NewRequest(call.method, t.url(call), nil)
	}
	if err != nil {
		return 0, nil, nil, clientError("client.invalid-request", "%v", err)
	}
	req = req.WithContext(reqCtx)
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", t.config.UserAgent)
	if t.config.Token != "" {
		req.Header.Set("Authorization", "Bearer "+t.config.Token)
	}
	if call.requestID != "" {
		req.Header.Set("X-Request-Id", call.requestID)
	}
	if call.traceparent != "" {
		req.Header.Set("traceparent", call.traceparent)
	}
	if call.hasBody {
		ct := call.contentType
		if ct == "" {
			ct = "application/json"
		}
		req.Header.Set("Content-Type", ct)
	}
	resp, err := t.client.Do(req)
	if err != nil {
		return 0, nil, nil, t.classify(ctx, reqCtx, err)
	}
	defer resp.Body.Close()
	data, err := ioutil.ReadAll(resp.Body)
	if err != nil {
		return 0, nil, nil, t.classify(ctx, reqCtx, err)
	}
	return resp.StatusCode, resp.Header, data, nil
}

func (t *transport) classify(ctx, reqCtx context.Context, err error) error {
	if ctx.Err() != nil {
		return ctx.Err()
	}
	dial, dialTimeout := isDialError(err)
	if dial && dialTimeout {
		return clientError("client.network", "timed out connecting to %s", t.base)
	}
	if reqCtx.Err() != nil || isTimeout(err) {
		return clientError("client.timeout", "timed out waiting for the response")
	}
	e := clientError("client.network", "%v", err)
	if dial {
		return &retryable{e}
	}
	return e
}

func errorCodeOf(body []byte) string {
	f, err := parseFields(body)
	if err != nil {
		return ""
	}
	code, _, _ := f.str("code")
	return code
}

func errorFrom(status int, headers http.Header, body []byte) *Error {
	text := string(body)
	e := &Error{Status: status, Body: body}
	if f, err := parseFields(body); err == nil {
		if code, ok, _ := f.str("code"); ok {
			e.Code = code
			e.Message, _, _ = f.str("message")
			e.RequestID, _, _ = f.str("requestId")
			if d := f.raw("details"); d != nil {
				e.Details = string(d)
			}
		}
	}
	if e.Code == "" {
		e.Code = "http." + strconv.Itoa(status)
		trimmed := strings.TrimSpace(text)
		if trimmed == "" {
			e.Message = "response " + strconv.Itoa(status) + " with no body"
		} else {
			if len(trimmed) > 300 {
				trimmed = trimmed[:300]
			}
			e.Message = trimmed
		}
	}
	if e.RequestID == "" {
		e.RequestID = headers.Get("X-Request-Id")
	}
	if h := strings.TrimSpace(headers.Get("Retry-After")); h != "" {
		if n, err := strconv.Atoi(h); err == nil && n >= 0 {
			e.RetryAfter, e.hasRetryAfter = n, true
		}
	}
	return e
}

// send makes the call, retrying only what the contract says to retry: 429 and 503 (waiting for the
// Retry-After, or 1 s, 2 s, 4 s), 502 and 504 only with retryGateway, and a failure to connect (refused, DNS).
// Never a 4xx (except 429), nor a failure after the body was sent, nor a timeout.
func (t *transport) send(ctx context.Context, call *apiCall) (*apiReply, error) {
	timeout := call.timeout
	if timeout <= 0 {
		timeout = t.config.RequestTimeout
	}
	maximum := t.config.MaxAttempts
	afterRetryable := false
	for attempt := 1; ; attempt++ {
		shift := uint(attempt - 1)
		if shift > 6 {
			shift = 6
		}
		backoff := time.Duration(1<<shift) * time.Second
		var wait time.Duration
		status, headers, body, err := t.once(ctx, call, timeout)
		if err != nil {
			r, ok := err.(*retryable)
			if !ok {
				return nil, err
			}
			if attempt >= maximum {
				return nil, r.err
			}
			afterRetryable = true
			wait = backoff
		} else {
			if status < 400 {
				return &apiReply{status: status, body: body}, nil
			}
			if afterRetryable {
				code := errorCodeOf(body)
				for _, rule := range call.recover {
					if rule.status == status && rule.code == code {
						return &apiReply{status: status, body: body, recovered: true}, nil
					}
				}
			}
			e := errorFrom(status, headers, body)
			retry := status == 429 || status == 503 || ((status == 502 || status == 504) && call.retryGateway)
			if !retry || attempt >= maximum {
				return nil, e
			}
			afterRetryable = afterRetryable || status == 503
			wait = backoff
			if e.hasRetryAfter {
				wait = time.Duration(e.RetryAfter) * time.Second
			}
		}
		if err := t.sleep(ctx, wait); err != nil {
			return nil, err
		}
	}
}
