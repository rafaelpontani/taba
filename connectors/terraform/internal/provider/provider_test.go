package provider

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/provider"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	rschema "github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/schema/validator"
	"github.com/hashicorp/terraform-plugin-framework/types"
	"github.com/hashicorp/terraform-plugin-go/tftypes"
)

func env(values map[string]string) func(string) string {
	return func(k string) string { return values[k] }
}

func nullProviderModel() providerModel {
	return providerModel{
		Endpoint: types.StringNull(), Token: types.StringNull(), CACertPEM: types.StringNull(),
		CACertFile: types.StringNull(), RequestTimeout: types.StringNull(), RetryMaxWait: types.StringNull(),
	}
}

// UC: UC-IAC-001
func TestTheSchemasAreValid(t *testing.T) {
	p := New("test")()
	var ps provider.SchemaResponse
	p.Schema(ctxb, provider.SchemaRequest{}, &ps)
	if d := ps.Schema.ValidateImplementation(ctxb); d.HasError() {
		t.Fatalf("provider: %v", d)
	}
	for _, f := range p.Resources(ctxb) {
		var rs resource.SchemaResponse
		f().Schema(ctxb, resource.SchemaRequest{}, &rs)
		if d := rs.Schema.ValidateImplementation(ctxb); d.HasError() {
			t.Fatalf("resource: %v", d)
		}
	}
	for _, f := range p.DataSources(ctxb) {
		var ds datasource.SchemaResponse
		f().Schema(ctxb, datasource.SchemaRequest{}, &ds)
		if d := ds.Schema.ValidateImplementation(ctxb); d.HasError() {
			t.Fatalf("data source: %v", d)
		}
	}
}

// UC: UC-IAC-001
func TestTheConfigurationFallsBackToTheEnvironmentAndAppliesTheDefaults(t *testing.T) {
	m := nullProviderModel()
	config, err := clientConfig(m, "1.2.3", env(map[string]string{envURL: "http://taba:8080", envToken: "tb_a.b"}))
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "http://taba:8080", config.BaseURL, "endpoint from TABA_URL")
	assertEqual(t, "tb_a.b", config.Token, "token from TABA_TOKEN")
	assertEqual(t, defaultRequestTimeout, config.RequestTimeout, "timeout")
	assertEqual(t, defaultRetryMaxWait, config.RetryMaxWait, "retry budget")
	assertEqual(t, retryAttempts, config.MaxAttempts, "attempts: the budget bounds them")
	assertEqual(t, "terraform-provider-taba/1.2.3", config.UserAgent, "user agent")
	if config.TLSConfig != nil {
		t.Error("no CA: the system roots")
	}

	m.Endpoint = types.StringValue("https://other:8080")
	m.RequestTimeout = types.StringValue("1m")
	m.RetryMaxWait = types.StringValue("90s")
	config, err = clientConfig(m, "1.2.3", env(map[string]string{envURL: "http://taba:8080"}))
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "https://other:8080", config.BaseURL, "the attribute wins over the environment")
	assertEqual(t, time.Minute, config.RequestTimeout, "timeout")
	assertEqual(t, 90*time.Second, config.RetryMaxWait, "retry budget")

	m.RetryMaxWait = types.StringValue("0s")
	config, err = clientConfig(m, "1.2.3", env(nil))
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, 1, config.MaxAttempts, "0s: no new attempt")
}

// UC: UC-IAC-001
func TestAnInvalidConfigurationIsRefused(t *testing.T) {
	cases := map[string]func(*providerModel){
		"without endpoint":        func(m *providerModel) {},
		"endpoint without scheme": func(m *providerModel) { m.Endpoint = types.StringValue("taba:8080") },
		"bad timeout": func(m *providerModel) {
			m.Endpoint = types.StringValue("http://x")
			m.RequestTimeout = types.StringValue("soon")
		},
		"negative retry_max_wait": func(m *providerModel) {
			m.Endpoint = types.StringValue("http://x")
			m.RetryMaxWait = types.StringValue("-1s")
		},
		"endpoint known at apply": func(m *providerModel) { m.Endpoint = types.StringUnknown() },
		"CA without a certificate": func(m *providerModel) {
			m.Endpoint = types.StringValue("http://x")
			m.CACertPEM = types.StringValue("nothing")
		},
		"CA file missing": func(m *providerModel) {
			m.Endpoint = types.StringValue("http://x")
			m.CACertFile = types.StringValue(filepath.Join(t.TempDir(), "none.pem"))
		},
	}
	for name, change := range cases {
		m := nullProviderModel()
		change(&m)
		if _, err := clientConfig(m, "t", env(nil)); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
}

// UC: UC-IAC-001
func TestTheCertificateAuthorityComesFromThePEMTheFileOrTheEnvironment(t *testing.T) {
	pem := testCA(t)
	file := filepath.Join(t.TempDir(), "ca.pem")
	if err := os.WriteFile(file, []byte(pem), 0o600); err != nil {
		t.Fatal(err)
	}
	for name, setup := range map[string]func(*providerModel) map[string]string{
		"ca_cert_pem":           func(m *providerModel) map[string]string { m.CACertPEM = types.StringValue(pem); return nil },
		"ca_cert_file":          func(m *providerModel) map[string]string { m.CACertFile = types.StringValue(file); return nil },
		"TABA_CA_CERT with PEM": func(m *providerModel) map[string]string { return map[string]string{envCACert: pem} },
		"TABA_CA_CERT path":     func(m *providerModel) map[string]string { return map[string]string{envCACert: file} },
	} {
		m := nullProviderModel()
		m.Endpoint = types.StringValue("https://taba:8080")
		config, err := clientConfig(m, "t", env(setup(&m)))
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if config.TLSConfig == nil || config.TLSConfig.RootCAs == nil {
			t.Errorf("%s: no CA in the TLS configuration", name)
		}
	}
}

func queueSchema(t *testing.T) rschema.Schema {
	t.Helper()
	var resp resource.SchemaResponse
	newQueueResource().Schema(ctxb, resource.SchemaRequest{}, &resp)
	return resp.Schema
}

// UC: UC-IAC-003
func TestTheTopicNameRulesAreCheckedAtPlan(t *testing.T) {
	for name, valid := range map[string]bool{
		"orders": true, "Orders_v2-x": true, strings.Repeat("a", 100): true,
		"": false, "with space": false, "dots.no": false, strings.Repeat("a", 101): false,
		"orders-dead-letter": false,
	} {
		var resp validator.StringResponse
		topicNameValidator{}.ValidateString(ctxb, validator.StringRequest{Path: path.Root("name"), ConfigValue: types.StringValue(name)}, &resp)
		if resp.Diagnostics.HasError() == valid {
			t.Errorf("%q: valid should be %v (%v)", name, valid, resp.Diagnostics)
		}
	}
}

// UC: UC-IAC-003
func TestIdleTimeoutNeedsAVolatileQueue(t *testing.T) {
	m := queueModel{IdleTimeout: types.Int64Value(120), Volatile: types.BoolNull()}
	if !validateQueue(m).HasError() {
		t.Error("idle_timeout on a durable queue was accepted")
	}
	m.Volatile = types.BoolValue(true)
	if validateQueue(m).HasError() {
		t.Error("idle_timeout on a volatile queue was refused")
	}
	m.Volatile = types.BoolUnknown()
	if validateQueue(m).HasError() {
		t.Error("an unknown volatile is checked at apply, not refused at plan")
	}
}

// UC: UC-IAC-003
func TestTheEffectiveValuesArePlannedFromTheFieldTheyFollow(t *testing.T) {
	s := queueSchema(t)
	modifier := effectiveModifier{field: "ttl"}
	plan := func(config, state map[string]tftypes.Value, stateValue types.Int64) types.Int64 {
		req := planmodifier.Int64Request{
			Config:     resourceConfig(t, s, config),
			State:      resourceState(t, s, state),
			StateValue: stateValue,
		}
		resp := &planmodifier.Int64Response{PlanValue: types.Int64Unknown()}
		modifier.PlanModifyInt64(ctxb, req, resp)
		if resp.Diagnostics.HasError() {
			t.Fatal(resp.Diagnostics)
		}
		return resp.PlanValue
	}
	ttl := func(n int64) map[string]tftypes.Value {
		return map[string]tftypes.Value{"ttl": tftypes.NewValue(tftypes.Number, n)}
	}

	assertEqual(t, types.Int64Value(3600), plan(ttl(3600), nil, types.Int64Null()), "declared: the declared value")
	assertEqual(t, true, plan(nil, nil, types.Int64Null()).IsUnknown(), "creating with the default: the server says")
	assertEqual(t, types.Int64Value(1209600), plan(nil, map[string]tftypes.Value{}, types.Int64Value(1209600)), "still the default: the state")
	assertEqual(t, true, plan(nil, ttl(3600), types.Int64Value(3600)).IsUnknown(), "back to the default: the server says")
}

// UC: UC-IAC-003
func TestIdleTimeoutNotDeclaredIsTheServerDefaultOnlyOnAVolatileQueue(t *testing.T) {
	s := queueSchema(t)
	plan := func(config map[string]tftypes.Value) types.Int64 {
		c := resourceConfig(t, s, config)
		var configured types.Int64
		c.GetAttribute(ctxb, path.Root("idle_timeout"), &configured)
		resp := &planmodifier.Int64Response{PlanValue: types.Int64Value(1200)}
		idleTimeoutModifier{}.PlanModifyInt64(ctxb, planmodifier.Int64Request{Config: c, ConfigValue: configured}, resp)
		return resp.PlanValue
	}
	volatile := tftypes.NewValue(tftypes.Bool, true)
	assertEqual(t, types.Int64Value(defaultIdleTimeout), plan(map[string]tftypes.Value{"volatile": volatile}), "volatile, not declared")
	assertEqual(t, types.Int64Null(), plan(nil), "durable")
	assertEqual(t, types.Int64Value(1200), plan(map[string]tftypes.Value{
		"volatile": volatile, "idle_timeout": tftypes.NewValue(tftypes.Number, 1200),
	}), "declared: unchanged")
}
