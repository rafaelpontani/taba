package taba

import (
	"context"
	"encoding/json"
	"io/ioutil"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

// A local HTTP server that records every request and answers from a script (the ScriptedHandler of the C# connector).

type recorded struct {
	Method string
	Path   string
	Query  map[string]string
	Raw    string
	Header http.Header
	Body   []byte
}

func (r recorded) text() string { return string(r.Body) }

type answer struct {
	status  int
	body    string
	headers map[string]string
	destroy bool
	hang    bool
}

func reply(status int, body interface{}) answer {
	switch b := body.(type) {
	case string:
		return answer{status: status, body: b}
	default:
		data, _ := json.Marshal(b)
		return answer{status: status, body: string(data)}
	}
}

func (a answer) with(key, value string) answer {
	headers := map[string]string{}
	for k, v := range a.headers {
		headers[k] = v
	}
	headers[key] = value
	a.headers = headers
	return a
}

var (
	destroyed = answer{destroy: true}
	hung      = answer{hang: true}
)

type scripted struct {
	server   *httptest.Server
	mu       sync.Mutex
	requests []recorded
	script   []func(recorded) answer
	fallback func(recorded) answer
	stop     chan struct{}
}

func newScripted(t *testing.T) *scripted {
	s := &scripted{stop: make(chan struct{})}
	s.fallback = func(recorded) answer { return reply(200, "{}") }
	s.server = httptest.NewServer(http.HandlerFunc(s.serve))
	return s
}

func (s *scripted) serve(w http.ResponseWriter, r *http.Request) {
	body, _ := ioutil.ReadAll(r.Body)
	query := map[string]string{}
	for k, v := range r.URL.Query() {
		query[k] = v[0]
	}
	rec := recorded{Method: r.Method, Path: r.URL.EscapedPath(), Query: query, Raw: r.URL.RawQuery, Header: r.Header, Body: body}
	s.mu.Lock()
	s.requests = append(s.requests, rec)
	next := s.fallback
	if len(s.script) > 0 {
		next = s.script[0]
		s.script = s.script[1:]
	}
	s.mu.Unlock()
	a := next(rec)
	if a.destroy {
		if hj, ok := w.(http.Hijacker); ok {
			conn, _, err := hj.Hijack()
			if err == nil {
				conn.Close()
			}
		}
		return
	}
	if a.hang {
		select {
		case <-s.stop:
		case <-time.After(30 * time.Second):
		}
		return
	}
	w.Header().Set("Content-Type", "application/json")
	for k, v := range a.headers {
		w.Header().Set(k, v)
	}
	w.WriteHeader(a.status)
	w.Write([]byte(a.body))
}

func (s *scripted) url() string { return s.server.URL }

func (s *scripted) reply(answers ...answer) *scripted {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, a := range answers {
		a := a
		s.script = append(s.script, func(recorded) answer { return a })
	}
	return s
}

func (s *scripted) replyWith(f func(recorded) answer) *scripted {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.script = append(s.script, f)
	return s
}

func (s *scripted) setFallback(a answer) *scripted {
	return s.setFallbackFunc(func(recorded) answer { return a })
}

func (s *scripted) setFallbackFunc(f func(recorded) answer) *scripted {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.fallback = f
	return s
}

// batches answers the consume calls (GET) with these message batches, in order, then with an empty one; the rest
// gets a plain 200.
func (s *scripted) batches(batches ...[]interface{}) *scripted {
	queue := batches
	return s.setFallbackFunc(func(r recorded) answer {
		if r.Method != "GET" {
			return reply(200, map[string]string{"id": "x", "invisibleUntil": "x"})
		}
		s.mu.Lock()
		defer s.mu.Unlock()
		var batch []interface{}
		if len(queue) > 0 {
			batch, queue = queue[0], queue[1:]
		}
		if batch == nil {
			batch = []interface{}{}
		}
		return reply(200, map[string]interface{}{"messages": batch})
	})
}

func (s *scripted) all() []recorded {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]recorded(nil), s.requests...)
}

func (s *scripted) count() int { return len(s.all()) }

func (s *scripted) close() {
	close(s.stop)
	s.server.CloseClientConnections()
	s.server.Close()
}

// fakeSleep does not wait and records what was asked.
type fakeSleep struct {
	mu    sync.Mutex
	waits []time.Duration
}

func (f *fakeSleep) sleep(ctx context.Context, d time.Duration) error {
	f.mu.Lock()
	f.waits = append(f.waits, d)
	f.mu.Unlock()
	return ctx.Err()
}

func (f *fakeSleep) recorded() []time.Duration {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]time.Duration(nil), f.waits...)
}

// blockingSleep never ends by itself: the renewal does not spin in tests that do not look at it.
func blockingSleep(ctx context.Context, d time.Duration) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-time.After(time.Minute):
		return nil
	}
}

type fixture struct {
	t      *testing.T
	server *scripted
	client *Client
	sleep  *fakeSleep
}

func newFixture(t *testing.T, mutate ...func(*Config)) *fixture {
	server := newScripted(t)
	config := Config{BaseURL: server.url()}
	for _, m := range mutate {
		m(&config)
	}
	client, err := NewClient(config)
	if err != nil {
		t.Fatal(err)
	}
	f := &fixture{t: t, server: server, client: client, sleep: &fakeSleep{}}
	client.t.sleep = f.sleep.sleep
	return f
}

func (f *fixture) close() {
	f.client.Close()
	f.server.close()
}

func assertEqual(t *testing.T, want, got interface{}, what string) {
	if want != got {
		t.Errorf("%s: want %v, got %v", what, want, got)
	}
}

func assertError(t *testing.T, code string, err error) *Error {
	e, ok := err.(*Error)
	if !ok {
		t.Fatalf("want a *Error %s, got %v", code, err)
	}
	if e.Code != code {
		t.Fatalf("want code %s, got %s (%s)", code, e.Code, e.Message)
	}
	return e
}

func topicJSON(name string, extra ...map[string]interface{}) map[string]interface{} {
	data := map[string]interface{}{"id": "t-" + name, "name": name, "type": "queue", "createdAt": "2026-10-27T10:00:00Z"}
	for _, e := range extra {
		for k, v := range e {
			data[k] = v
		}
	}
	return data
}

func rawJSON(text string) *json.RawMessage {
	raw := json.RawMessage(text)
	return &raw
}

// messageJSON builds a message; a string body is JSON text already written.
func messageJSON(id string, body interface{}, extra ...map[string]interface{}) map[string]interface{} {
	if text, ok := body.(string); ok {
		body = rawJSON(text)
	}
	data := map[string]interface{}{
		"id": id, "body": body, "contentType": "application/json", "receiptToken": "rt-" + id,
		"deliveryCount": 1, "publishedAt": "2026-10-27T10:00:00Z", "invisibleUntil": "2026-10-27T10:02:00Z",
	}
	for _, e := range extra {
		for k, v := range e {
			data[k] = v
		}
	}
	return data
}

func waitFor(t *testing.T, condition func() bool) {
	deadline := time.Now().Add(5 * time.Second)
	for !condition() {
		if time.Now().After(deadline) {
			t.Fatal("condition not met in 5 s")
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func contains(s, sub string) bool { return strings.Contains(s, sub) }
