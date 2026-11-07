package provider

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/json"
	"encoding/pem"
	"io"
	"math/big"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"

	"github.com/hashicorp/terraform-plugin-framework/tfsdk"
	"github.com/hashicorp/terraform-plugin-go/tftypes"

	rschema "github.com/hashicorp/terraform-plugin-framework/resource/schema"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

var ctxb = context.Background()

// recorded is one request the fake server received.
type recorded struct {
	Method string
	Path   string
	Query  string
	Body   string
}

// answer is one scripted response.
type answer struct {
	status  int
	body    string
	headers map[string]string
}

// fakeServer answers the scripted responses in order (the last one repeats) and records every request.
type fakeServer struct {
	t        *testing.T
	server   *httptest.Server
	mu       sync.Mutex
	answers  []answer
	requests []recorded
}

func newFakeServer(t *testing.T, answers ...answer) (*fakeServer, *taba.Client) {
	t.Helper()
	f := &fakeServer{t: t, answers: answers}
	f.server = httptest.NewServer(http.HandlerFunc(f.handle))
	t.Cleanup(f.server.Close)
	client, err := taba.NewClient(taba.Config{BaseURL: f.server.URL, MaxAttempts: 3, RetryMaxWait: 6 * time.Second})
	if err != nil {
		t.Fatal(err)
	}
	return f, client
}

func (f *fakeServer) handle(w http.ResponseWriter, r *http.Request) {
	body, _ := io.ReadAll(r.Body)
	f.mu.Lock()
	f.requests = append(f.requests, recorded{Method: r.Method, Path: r.URL.EscapedPath(), Query: r.URL.RawQuery, Body: string(body)})
	a := f.answers[0]
	if len(f.answers) > 1 {
		f.answers = f.answers[1:]
	}
	f.mu.Unlock()
	for k, v := range a.headers {
		w.Header().Set(k, v)
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(a.status)
	_, _ = io.WriteString(w, a.body)
}

func (f *fakeServer) all() []recorded {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]recorded(nil), f.requests...)
}

func ok(status int, value any) answer {
	b, _ := json.Marshal(value)
	return answer{status: status, body: string(b)}
}

func failure(status int, code string) answer {
	return answer{status: status, body: `{"code":"` + code + `","message":"x"}`}
}

// unavailable is a 503 that the connector repeats at once (Retry-After: 0).
func unavailable(code string) answer {
	a := failure(503, code)
	a.headers = map[string]string{"Retry-After": "0"}
	return a
}

// queueJSON is a queue as the server returns it, with the node defaults in force.
func queueJSON(name string, extra map[string]any) map[string]any {
	data := map[string]any{
		"id": "id-" + name, "name": name, "type": "queue", "createdAt": "2026-11-07T10:00:00Z",
		"fifo": false, "volatile": false, "tags": map[string]string{},
		"ttl": nil, "maxDeliveries": nil, "maxMessageBytes": nil,
		"effective": map[string]int{"ttl": 1209600, "maxDeliveries": 10, "maxMessageBytes": 5242880},
	}
	for k, v := range extra {
		data[k] = v
	}
	return data
}

func distributorJSON(name string, prefixes []string) map[string]any {
	return map[string]any{
		"id": "id-" + name, "name": name, "type": "distributor", "createdAt": "2026-11-07T10:00:00Z",
		"tags": map[string]string{}, "prefixes": prefixes,
	}
}

func assertEqual(t *testing.T, want, got any, what string) {
	t.Helper()
	if want != got {
		t.Errorf("%s: want %v, got %v", what, want, got)
	}
}

func assertJSON(t *testing.T, want, got, what string) {
	t.Helper()
	var w, g any
	if err := json.Unmarshal([]byte(want), &w); err != nil {
		t.Fatalf("%s: bad expected JSON: %v", what, err)
	}
	if err := json.Unmarshal([]byte(got), &g); err != nil {
		t.Fatalf("%s: the body is not JSON: %q", what, got)
	}
	wb, _ := json.Marshal(w)
	gb, _ := json.Marshal(g)
	if string(wb) != string(gb) {
		t.Errorf("%s:\nwant %s\ngot  %s", what, wb, gb)
	}
}

// resourceObject builds a value of the resource schema: the given attributes, every other one null.
func resourceObject(t *testing.T, s rschema.Schema, values map[string]tftypes.Value) tftypes.Value {
	t.Helper()
	objectType, ok := s.Type().TerraformType(ctxb).(tftypes.Object)
	if !ok {
		t.Fatal("the schema is not an object")
	}
	all := map[string]tftypes.Value{}
	for name, attrType := range objectType.AttributeTypes {
		if v, ok := values[name]; ok {
			all[name] = v
		} else {
			all[name] = tftypes.NewValue(attrType, nil)
		}
	}
	return tftypes.NewValue(objectType, all)
}

func resourceConfig(t *testing.T, s rschema.Schema, values map[string]tftypes.Value) tfsdk.Config {
	return tfsdk.Config{Schema: s, Raw: resourceObject(t, s, values)}
}

func resourceState(t *testing.T, s rschema.Schema, values map[string]tftypes.Value) tfsdk.State {
	if values == nil {
		objectType := s.Type().TerraformType(ctxb)
		return tfsdk.State{Schema: s, Raw: tftypes.NewValue(objectType, nil)}
	}
	return tfsdk.State{Schema: s, Raw: resourceObject(t, s, values)}
}

// testCA is the PEM of a self-signed certificate, as a private authority.
func testCA(t *testing.T) string {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	template := &x509.Certificate{
		SerialNumber:          big.NewInt(1),
		Subject:               pkix.Name{CommonName: "taba test CA"},
		NotBefore:             time.Now().Add(-time.Hour),
		NotAfter:              time.Now().Add(time.Hour),
		IsCA:                  true,
		BasicConstraintsValid: true,
		KeyUsage:              x509.KeyUsageCertSign,
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		t.Fatal(err)
	}
	return string(pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der}))
}
