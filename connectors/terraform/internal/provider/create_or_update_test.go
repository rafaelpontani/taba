package provider

import (
	"strings"
	"testing"

	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/types"
)

func warningText(d diag.Diagnostics) string {
	var b strings.Builder
	for _, w := range d.Warnings() {
		b.WriteString(w.Summary() + ": " + w.Detail() + "\n")
	}
	return b.String()
}

func recreated(value any) answer {
	a := ok(201, value)
	a.headers = map[string]string{"Taba-Topic-Recreated": "true"}
	return a
}

// -- the provider default and the resource attribute ---------------------------------------------------------------

// UC: UC-IAC-018
func TestCreateOrUpdateComesFromTheProviderOrTheEnvironmentAndTheResourceWins(t *testing.T) {
	m := nullProviderModel()
	on, err := createOrUpdateDefault(m, env(nil))
	assertEqual(t, nil, err, "error")
	assertEqual(t, false, on, "off by default")

	on, _ = createOrUpdateDefault(m, env(map[string]string{envCreateOrUpdate: "true"}))
	assertEqual(t, true, on, "TABA_CREATE_OR_UPDATE")

	m.CreateOrUpdate = types.BoolValue(false)
	on, _ = createOrUpdateDefault(m, env(map[string]string{envCreateOrUpdate: "true"}))
	assertEqual(t, false, on, "the attribute wins over the environment")

	if _, err := createOrUpdateDefault(nullProviderModel(), env(map[string]string{envCreateOrUpdate: "yes please"})); err == nil {
		t.Error("an invalid TABA_CREATE_OR_UPDATE was accepted")
	}
	m.CreateOrUpdate = types.BoolUnknown()
	if _, err := createOrUpdateDefault(m, env(nil)); err == nil {
		t.Error("a value known only at apply was accepted")
	}

	assertEqual(t, true, wantsCreateOrUpdate(types.BoolNull(), true), "not set: the provider")
	assertEqual(t, false, wantsCreateOrUpdate(types.BoolValue(false), true), "the resource wins (off)")
	assertEqual(t, true, wantsCreateOrUpdate(types.BoolValue(true), false), "the resource wins (on)")
}

// -- queues and distributors ----------------------------------------------------------------------------------------

// UC: UC-IAC-019
func TestWithCreateOrUpdateAnExistingQueueIsTakenOverWithAPut(t *testing.T) {
	f, c := newFakeServer(t, ok(200, queueJSON("orders", map[string]any{"ttl": 3600})))
	plan := declaredQueue("orders")
	plan.TTL = types.Int64Value(3600)
	plan.CreateOrUpdate = types.BoolValue(true)
	m, diags := createQueue(ctxb, c, plan, true)
	if diags.HasError() {
		t.Fatal(errorText(diags))
	}
	requests := f.all()
	assertEqual(t, "PUT /topics/orders", methods(requests), "one PUT, decided by the owner")
	assertEqual(t, "recreate=if-empty", requests[0].Query, "never recreate=always")
	assertJSON(t, `{"type":"queue","ttl":3600}`, requests[0].Body, "the whole declared state, without the name")
	if !strings.Contains(warningText(diags), "Took over the existing queue") {
		t.Errorf("no warning of the takeover: %q", warningText(diags))
	}
	assertEqual(t, "id-orders", m.ID.ValueString(), "the existing id")
	assertEqual(t, int64(3600), m.TTL.ValueInt64(), "ttl")
	assertEqual(t, true, m.CreateOrUpdate.ValueBool(), "the attribute stays as declared")
}

// UC: UC-IAC-019
func TestWithCreateOrUpdateAMissingQueueIsCreatedWithoutAWarning(t *testing.T) {
	_, c := newFakeServer(t, ok(201, queueJSON("orders", nil)))
	_, diags := createQueue(ctxb, c, declaredQueue("orders"), true)
	if diags.HasError() || len(diags.Warnings()) > 0 {
		t.Fatalf("errors %q, warnings %q", errorText(diags), warningText(diags))
	}
}

// UC: UC-IAC-020
func TestWithCreateOrUpdateAnEmptyTopicOfAnotherKindIsRecreatedWithAWarning(t *testing.T) {
	_, c := newFakeServer(t, recreated(queueJSON("orders", map[string]any{"id": "new"})))
	m, diags := createQueue(ctxb, c, declaredQueue("orders"), true)
	if diags.HasError() {
		t.Fatal(errorText(diags))
	}
	if !strings.Contains(warningText(diags), "Recreated the topic") {
		t.Errorf("no warning of the recreation: %q", warningText(diags))
	}
	assertEqual(t, "new", m.ID.ValueString(), "the new id")
}

// UC: UC-IAC-020
func TestWithCreateOrUpdateATopicOfAnotherKindWithMessagesIsNotTouched(t *testing.T) {
	notEmpty := answer{status: 409, body: `{"code":"topic.not-empty","message":"x","details":[{"field":"messages","message":"the queue holds 3 messages"}]}`}
	f, c := newFakeServer(t, notEmpty)
	_, diags := createQueue(ctxb, c, declaredQueue("orders"), true)
	text := errorText(diags)
	for _, want := range []string{"is not empty", "terraform import", "force_destroy", "holds 3 messages"} {
		if !strings.Contains(text, want) {
			t.Errorf("the error lacks %q: %s", want, text)
		}
	}
	assertEqual(t, "PUT /topics/orders", methods(f.all()), "nothing after the refusal")
}

// UC: UC-IAC-019
func TestWithCreateOrUpdateAPutRefusedForAMissingCountSaysNothingChanged(t *testing.T) {
	_, c := newFakeServer(t, failure(503, "cluster.quorum-unavailable"))
	_, diags := createQueue(ctxb, c, declaredQueue("orders"), true)
	if !strings.Contains(errorText(diags), "nothing was changed") {
		t.Errorf("error: %s", errorText(diags))
	}
}

// UC: UC-IAC-019
func TestWithCreateOrUpdateAConflictIsRepeated(t *testing.T) {
	conflict := failure(409, "cluster.conflict")
	conflict.headers = map[string]string{"Retry-After": "1"}
	f, c := newFakeServer(t, conflict, ok(200, queueJSON("orders", nil)))
	_, diags := createQueue(ctxb, c, declaredQueue("orders"), true)
	if diags.HasError() {
		t.Fatal(errorText(diags))
	}
	assertEqual(t, "PUT /topics/orders, PUT /topics/orders", methods(f.all()), "the PUT again")
}

// UC: UC-IAC-009
func TestWithoutCreateOrUpdateAnExistingTopicIsAnErrorThatNamesBothWays(t *testing.T) {
	_, c := newFakeServer(t, failure(409, "topic.already-exists"))
	_, diags := createQueue(ctxb, c, declaredQueue("orders"), false)
	text := errorText(diags)
	for _, want := range []string{"terraform import", "create_or_update = true"} {
		if !strings.Contains(text, want) {
			t.Errorf("the error lacks %q: %s", want, text)
		}
	}
	_, c = newFakeServer(t, failure(409, "topic.already-exists"))
	_, diags = createDistributor(ctxb, c, declaredDistributor("fanout", "orders-"), false)
	if !strings.Contains(errorText(diags), "create_or_update = true") {
		t.Errorf("distributor: %s", errorText(diags))
	}
}

// UC: UC-IAC-019
func TestWithCreateOrUpdateAnExistingDistributorIsTakenOverWithAPut(t *testing.T) {
	f, c := newFakeServer(t, ok(200, distributorJSON("fanout", []string{"orders-"})))
	plan := declaredDistributor("fanout", "orders-")
	plan.CreateOrUpdate = types.BoolValue(true)
	m, diags := createDistributor(ctxb, c, plan, true)
	if diags.HasError() {
		t.Fatal(errorText(diags))
	}
	requests := f.all()
	assertEqual(t, "PUT /topics/fanout", methods(requests), "one PUT")
	assertEqual(t, "recreate=if-empty", requests[0].Query, "query")
	assertJSON(t, `{"type":"distributor","prefixes":["orders-"]}`, requests[0].Body, "body: empty tags left out (the PUT takes them to {})")
	if !strings.Contains(warningText(diags), "Took over the existing distributor") {
		t.Errorf("no warning: %q", warningText(diags))
	}
	assertEqual(t, true, m.CreateOrUpdate.ValueBool(), "the attribute stays as declared")
}

// -- tokens ---------------------------------------------------------------------------------------------------------

// UC: UC-IAC-021
func TestWithCreateOrUpdateATokenWithoutTheExactNameIsCreated(t *testing.T) {
	f, c := newFakeServer(t,
		page(tokenJSON("old", "ci-old", []string{"publish"}, nil, nil)),
		ok(201, tokenCreatedJSON("t1", "ci", []string{"publish"}, nil, nil)),
	)
	m, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), true)
	if diags.HasError() {
		t.Fatal(errorText(diags))
	}
	assertEqual(t, "GET /tokens, POST /tokens", methods(f.all()), "the name is a part of another: created")
	assertEqual(t, "tb_t1.secret", m.Token.ValueString(), "secret")
	if len(diags.Warnings()) > 0 {
		t.Errorf("warning: %s", warningText(diags))
	}
}

// UC: UC-IAC-021
func TestWithCreateOrUpdateTheOnlyTokenWithTheNameIsTakenOverWithoutTheSecret(t *testing.T) {
	f, c := newFakeServer(t, page(tokenJSON("t1", "ci", []string{"consume", "publish"}, []string{"orders"}, "2027-01-01T00:00:00.000Z")))
	plan := plannedToken("ci", []string{"publish", "consume"}, []string{"orders"}, types.StringValue("2027-01-01T00:00:00Z"))
	plan.CreateOrUpdate = types.BoolValue(true)
	m, diags := createToken(ctxb, c, plan, true)
	if diags.HasError() {
		t.Fatal(errorText(diags))
	}
	assertEqual(t, "GET /tokens", methods(f.all()), "same deadline: no PATCH, no POST")
	assertEqual(t, "t1", m.ID.ValueString(), "the existing id")
	assertEqual(t, true, m.Token.IsNull(), "the secret cannot be recovered")
	assertEqual(t, "2027-01-01T00:00:00Z", m.ExpiresAt.ValueString(), "the declared text of the same instant")
	assertEqual(t, true, m.CreateOrUpdate.ValueBool(), "the attribute stays as declared")
	if !strings.Contains(warningText(diags), "-replace") {
		t.Errorf("the warning does not say how to get a secret: %q", warningText(diags))
	}
}

// UC: UC-IAC-021
func TestATokenTakenOverGetsTheDeclaredDeadline(t *testing.T) {
	f, c := newFakeServer(t,
		page(tokenJSON("t1", "ci", []string{"publish"}, nil, nil)),
		ok(200, tokenJSON("t1", "ci", []string{"publish"}, nil, "2027-06-01T00:00:00.000Z")),
	)
	m, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringValue("2027-06-01T00:00:00Z")), true)
	if diags.HasError() {
		t.Fatal(errorText(diags))
	}
	requests := f.all()
	assertEqual(t, "GET /tokens, PATCH /tokens/t1", methods(requests), "calls")
	assertJSON(t, `{"expiresAt":"2027-06-01T00:00:00.000Z"}`, requests[1].Body, "only the deadline")
	assertEqual(t, "2027-06-01T00:00:00Z", m.ExpiresAt.ValueString(), "deadline")

	f, c = newFakeServer(t,
		page(tokenJSON("t1", "ci", []string{"publish"}, nil, "2027-06-01T00:00:00.000Z")),
		ok(200, tokenJSON("t1", "ci", []string{"publish"}, nil, nil)),
	)
	m, diags = createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), true)
	if diags.HasError() {
		t.Fatal(errorText(diags))
	}
	assertJSON(t, `{"expiresAt":null}`, f.all()[1].Body, "no deadline declared: removed")
	assertEqual(t, true, m.ExpiresAt.IsNull(), "no deadline")
}

// UC: UC-IAC-022
func TestSeveralTokensWithTheNameAreNotTakenOver(t *testing.T) {
	f, c := newFakeServer(t, page(
		tokenJSON("t2", "ci", []string{"publish"}, nil, nil),
		tokenJSON("t1", "ci", []string{"publish"}, nil, nil),
	))
	_, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), true)
	if !strings.Contains(errorText(diags), "t1, t2") {
		t.Errorf("the error does not name the tokens: %s", errorText(diags))
	}
	assertEqual(t, "GET /tokens", methods(f.all()), "nothing changed or created")
}

// UC: UC-IAC-022
func TestATokenWithOtherPermissionsOrPrefixesIsNotTakenOver(t *testing.T) {
	for name, existing := range map[string]map[string]any{
		"permissions": tokenJSON("t1", "ci", []string{"consume", "publish"}, nil, nil),
		"prefixes":    tokenJSON("t1", "ci", []string{"publish"}, []string{"orders"}, nil),
	} {
		f, c := newFakeServer(t, page(existing))
		_, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), true)
		if !strings.Contains(errorText(diags), "other permissions or prefixes") {
			t.Errorf("%s: %s", name, errorText(diags))
		}
		assertEqual(t, "GET /tokens", methods(f.all()), name+": nothing changed or created")
	}
}

// UC: UC-IAC-021
func TestATokenThatDisappearsWhileTakenOverIsAnError(t *testing.T) {
	_, c := newFakeServer(t, page(tokenJSON("t1", "ci", []string{"publish"}, nil, nil)), failure(404, "token.not-found"))
	_, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringValue("2027-06-01T00:00:00Z")), true)
	if !strings.Contains(errorText(diags), "no longer exists") {
		t.Errorf("error: %s", errorText(diags))
	}
}
