package provider

import (
	"context"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"
)

func tokenJSON(id, name string, permissions, prefixes []string, expiresAt any) map[string]any {
	if prefixes == nil {
		prefixes = []string{}
	}
	return map[string]any{
		"id": id, "name": name, "permissions": permissions, "topicPrefixes": prefixes,
		"createdAt": "2026-11-07T10:00:00.000Z", "expiresAt": expiresAt,
	}
}

func tokenCreatedJSON(id, name string, permissions, prefixes []string, expiresAt any) map[string]any {
	data := tokenJSON(id, name, permissions, prefixes, expiresAt)
	data["token"] = "tb_" + id + ".secret"
	return data
}

func page(items ...map[string]any) answer {
	if items == nil {
		items = []map[string]any{}
	}
	return ok(200, map[string]any{"items": items, "nextCursor": nil})
}

func plannedToken(name string, permissions, prefixes []string, expiresAt types.String) tokenModel {
	return tokenModel{
		ID: types.StringUnknown(), Name: types.StringValue(name),
		Permissions: setValue(permissions), TopicPrefixes: setValue(prefixes),
		ExpiresAt: expiresAt, CreatedAt: types.StringUnknown(), Token: types.StringUnknown(),
	}
}

func errorText(d diag.Diagnostics) string {
	var b strings.Builder
	for _, e := range d.Errors() {
		b.WriteString(e.Summary() + ": " + e.Detail() + "\n")
	}
	return b.String()
}

func methods(requests []recorded) string {
	parts := make([]string, 0, len(requests))
	for _, r := range requests {
		parts = append(parts, r.Method+" "+r.Path)
	}
	return strings.Join(parts, ", ")
}

// UC: UC-IAC-013
func TestCreatingATokenSendsTheDeclaredTokenAndKeepsTheSecret(t *testing.T) {
	f, c := newFakeServer(t,
		page(),
		ok(201, tokenCreatedJSON("t1", "ci", []string{"consume", "publish"}, []string{"orders"}, "2027-01-01T00:00:00.000Z")),
	)
	plan := plannedToken("ci", []string{"publish", "consume"}, []string{"orders"}, types.StringValue("2027-01-01T03:00:00+03:00"))
	m, diags := createToken(ctxb, c, plan, false)
	if diags.HasError() {
		t.Fatal(diags)
	}
	requests := f.all()
	assertEqual(t, "GET /tokens, POST /tokens", methods(requests), "calls")
	assertEqual(t, "name=ci", requests[0].Query, "the list before, by name")
	assertJSON(t, `{"name":"ci","permissions":["consume","publish"],"topicPrefixes":["orders"],"expiresAt":"2027-01-01T00:00:00.000Z"}`,
		requests[1].Body, "body: the date in the server format")
	assertEqual(t, "t1", m.ID.ValueString(), "id")
	assertEqual(t, "tb_t1.secret", m.Token.ValueString(), "secret")
	assertEqual(t, "2027-01-01T03:00:00+03:00", m.ExpiresAt.ValueString(), "the declared text of the same instant")
	assertEqual(t, true, m.Permissions.Equal(plan.Permissions), "permissions")
}

// UC: UC-IAC-013
func TestATokenWithoutDeadlineOrPrefixesLeavesThemOut(t *testing.T) {
	f, c := newFakeServer(t, page(), ok(201, tokenCreatedJSON("t1", "ci", []string{"admin"}, nil, nil)))
	m, diags := createToken(ctxb, c, plannedToken("ci", []string{"admin"}, nil, types.StringNull()), false)
	if diags.HasError() {
		t.Fatal(diags)
	}
	assertJSON(t, `{"name":"ci","permissions":["admin"]}`, f.all()[1].Body, "body")
	assertEqual(t, true, m.ExpiresAt.IsNull(), "no deadline")
	assertEqual(t, 0, len(m.TopicPrefixes.Elements()), "no prefixes")
}

// UC: UC-IAC-016
func TestTokensWithAuthenticationOffOrAWeakTokenGiveAHint(t *testing.T) {
	for code, want := range map[string]string{
		"route.not-found":   "authentication on",
		"auth.forbidden":    "root token",
		"auth.unauthorized": "did not accept",
	} {
		status := map[string]int{"route.not-found": 404, "auth.forbidden": 403, "auth.unauthorized": 401}[code]
		f, c := newFakeServer(t, failure(status, code))
		_, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), false)
		if !strings.Contains(errorText(diags), want) {
			t.Errorf("%s: the error does not say %q: %s", code, want, errorText(diags))
		}
		assertEqual(t, "GET /tokens", methods(f.all()), code+": nothing created")
	}
}

// UC: UC-IAC-017
func TestALostCreationAnswerRevokesTheOrphanAndCreatesAgain(t *testing.T) {
	other := tokenJSON("t0", "ci", []string{"publish"}, nil, nil)
	f, c := newFakeServer(t,
		page(other),
		failure(503, quorumUnavailable),
		page(other, tokenJSON("t1", "ci", []string{"publish"}, nil, nil), tokenJSON("t2", "ci", []string{"consume"}, nil, nil)),
		answer{status: 204},
		ok(201, tokenCreatedJSON("t3", "ci", []string{"publish"}, nil, nil)),
	)
	m, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), false)
	if diags.HasError() {
		t.Fatal(diags)
	}
	assertEqual(t, "GET /tokens, POST /tokens, GET /tokens, DELETE /tokens/t1, POST /tokens", methods(f.all()),
		"only the new equal token (t1) is revoked: not the one that existed (t0) nor the one with other permissions (t2)")
	assertEqual(t, "tb_t3.secret", m.Token.ValueString(), "the secret of the new one")
}

// UC: UC-IAC-017
func TestALostCreationAnswerWithTwoCandidatesRevokesNothing(t *testing.T) {
	f, c := newFakeServer(t,
		page(),
		failure(503, quorumUnavailable),
		page(tokenJSON("t2", "ci", []string{"publish"}, nil, nil), tokenJSON("t1", "ci", []string{"publish"}, nil, nil)),
	)
	_, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), false)
	if !strings.Contains(errorText(diags), "t1, t2") {
		t.Errorf("the error does not name the tokens: %s", errorText(diags))
	}
	assertEqual(t, "GET /tokens, POST /tokens, GET /tokens", methods(f.all()), "nothing revoked")
}

// noTokenWait replaces the wait between the creations of a token for the test, keeping what would be waited.
func noTokenWait(t *testing.T) *[]time.Duration {
	t.Helper()
	var waits []time.Duration
	previous := tokenRetryWait
	tokenRetryWait = func(_ context.Context, d time.Duration) error {
		waits = append(waits, d)
		return nil
	}
	t.Cleanup(func() { tokenRetryWait = previous })
	return &waits
}

// A node that went down is a member (and the owner of some definitions) until it is voted out: the creation is
// repeated with growing waits within retry_max_wait (6 s in the fake server's client: 1 s, then the Retry-After of
// 5 s; the next 4 s would pass it), and then it stops after revoking.
// UC: UC-IAC-017
func TestLostCreationAnswersAreRepeatedWithinRetryMaxWaitAndStopAfterRevoking(t *testing.T) {
	waits := noTokenWait(t)
	lost := failure(503, quorumUnavailable)
	f, c := newFakeServer(t,
		page(),
		lost,
		page(),
		answer{status: 503, body: lost.body, headers: map[string]string{"Retry-After": "5"}},
		page(),
		lost,
		page(tokenJSON("t1", "ci", []string{"publish"}, nil, nil)),
		answer{status: 204},
	)
	_, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), false)
	if !strings.Contains(errorText(diags), "did not confirm the creation in 3 attempts within retry_max_wait") {
		t.Errorf("error: %s", errorText(diags))
	}
	assertEqual(t, "GET /tokens, POST /tokens, GET /tokens, POST /tokens, GET /tokens, POST /tokens, GET /tokens, DELETE /tokens/t1",
		methods(f.all()), "calls")
	assertEqual(t, "[1s 5s]", fmt.Sprint(*waits), "1 s, then the Retry-After")
}

// UC: UC-IAC-017
func TestALostCreationAnswerThenTheNodeIsBackCreatesTheToken(t *testing.T) {
	waits := noTokenWait(t)
	f, c := newFakeServer(t,
		page(),
		failure(503, quorumUnavailable),
		page(),
		failure(503, quorumUnavailable),
		page(),
		ok(201, tokenCreatedJSON("t1", "ci", []string{"publish"}, nil, nil)),
	)
	m, diags := createToken(ctxb, c, plannedToken("ci", []string{"publish"}, nil, types.StringNull()), false)
	if diags.HasError() {
		t.Fatal(diags)
	}
	assertEqual(t, "tb_t1.secret", m.Token.ValueString(), "secret")
	assertEqual(t, "GET /tokens, POST /tokens, GET /tokens, POST /tokens, GET /tokens, POST /tokens", methods(f.all()), "calls")
	assertEqual(t, "[1s 2s]", fmt.Sprint(*waits), "waits")
}

// UC: UC-IAC-014, UC-IAC-016
func TestReadingATokenThatIsGoneRemovesItButAuthenticationOffIsAnError(t *testing.T) {
	state := plannedToken("ci", []string{"publish"}, nil, types.StringNull())
	state.ID = types.StringValue("t1")
	_, c := newFakeServer(t, failure(404, "token.not-found"))
	_, found, diags := readToken(ctxb, c, state)
	assertEqual(t, false, found || diags.HasError(), "revoked or expired: removed from the state")

	_, c = newFakeServer(t, failure(404, "route.not-found"))
	_, _, diags = readToken(ctxb, c, state)
	if !strings.Contains(errorText(diags), "authentication on") {
		t.Errorf("authentication off must not look like a revoked token: %s", errorText(diags))
	}
}

// UC: UC-IAC-013
func TestReadingATokenKeepsTheSecretAndTheDeclaredDate(t *testing.T) {
	state := plannedToken("ci", []string{"publish"}, nil, types.StringValue("2027-01-01T00:00:00Z"))
	state.ID = types.StringValue("t1")
	state.Token = types.StringValue("tb_t1.secret")
	_, c := newFakeServer(t, ok(200, tokenJSON("t1", "ci-renamed", []string{"publish"}, nil, "2027-01-01T00:00:00.000Z")))
	m, found, diags := readToken(ctxb, c, state)
	if !found || diags.HasError() {
		t.Fatal(diags)
	}
	assertEqual(t, "tb_t1.secret", m.Token.ValueString(), "secret")
	assertEqual(t, "2027-01-01T00:00:00Z", m.ExpiresAt.ValueString(), "same instant: the declared text")
	assertEqual(t, "ci-renamed", m.Name.ValueString(), "drift of the name")

	_, c = newFakeServer(t, ok(200, tokenJSON("t1", "ci", []string{"publish"}, nil, "2028-01-01T00:00:00.000Z")))
	m, _, _ = readToken(ctxb, c, state)
	assertEqual(t, "2028-01-01T00:00:00.000Z", m.ExpiresAt.ValueString(), "another instant: the server's")
}

// UC: UC-IAC-014
func TestUpdatingATokenPatchesOnlyWhatChanged(t *testing.T) {
	state := plannedToken("ci", []string{"publish"}, nil, types.StringValue("2027-01-01T00:00:00Z"))
	state.ID = types.StringValue("t1")
	state.Token = types.StringValue("tb_t1.secret")
	state.CreatedAt = types.StringValue("2026-11-07T10:00:00.000Z")

	plan := state
	plan.ExpiresAt = types.StringNull()
	f, c := newFakeServer(t, ok(200, tokenJSON("t1", "ci", []string{"publish"}, nil, nil)))
	m, diags := updateToken(ctxb, c, plan, state)
	if diags.HasError() {
		t.Fatal(diags)
	}
	assertEqual(t, "PATCH /tokens/t1", methods(f.all()), "call")
	assertJSON(t, `{"expiresAt":null}`, f.all()[0].Body, "removing the deadline sends null")
	assertEqual(t, "tb_t1.secret", m.Token.ValueString(), "the secret stays")
	assertEqual(t, true, m.ExpiresAt.IsNull(), "no deadline")

	plan = state
	plan.Name = types.StringValue("ci-2")
	plan.ExpiresAt = types.StringValue("2028-06-01T12:00:00-03:00")
	f, c = newFakeServer(t, ok(200, tokenJSON("t1", "ci-2", []string{"publish"}, nil, "2028-06-01T15:00:00.000Z")))
	m, diags = updateToken(ctxb, c, plan, state)
	if diags.HasError() {
		t.Fatal(diags)
	}
	assertJSON(t, `{"name":"ci-2","expiresAt":"2028-06-01T15:00:00.000Z"}`, f.all()[0].Body, "name and date")
	assertEqual(t, "2028-06-01T12:00:00-03:00", m.ExpiresAt.ValueString(), "the declared text")

	f, c = newFakeServer(t, failure(404, "token.not-found"))
	_, diags = updateToken(ctxb, c, plan, state)
	if !strings.Contains(errorText(diags), "no longer exists") {
		t.Errorf("error: %s", errorText(diags))
	}
	_ = f
}

// UC: UC-IAC-014
func TestRevokingATokenAlreadyGoneSucceeds(t *testing.T) {
	_, c := newFakeServer(t, failure(404, "token.not-found"))
	if diags := revokeToken(ctxb, c, "t1"); diags.HasError() {
		t.Error(diags)
	}
	_, c = newFakeServer(t, failure(404, "route.not-found"))
	if diags := revokeToken(ctxb, c, "t1"); !diags.HasError() {
		t.Error("authentication off: an error")
	}
}

// UC: UC-IAC-015
func TestTheTokenConfigurationIsCheckedAtPlan(t *testing.T) {
	now := time.Date(2026, 11, 7, 12, 0, 0, 0, time.UTC)
	check := func(m tokenModel) string { return errorText(validateToken(m, now)) }

	assertEqual(t, "", check(plannedToken("ci", []string{"publish"}, []string{"orders"}, types.StringValue("2026-11-08T00:00:00Z"))), "valid")
	if !strings.Contains(check(plannedToken("ci", []string{"admin"}, []string{"orders"}, types.StringNull())), "admin reaches every topic") {
		t.Error("admin with prefixes accepted")
	}
	if !strings.Contains(check(plannedToken("ci", []string{"publish"}, nil, types.StringValue("2026-11-07T11:59:59Z"))), "The token has expired") {
		t.Error("a deadline in the past accepted")
	}
	if !strings.Contains(check(plannedToken("ci", []string{"publish"}, nil, types.StringValue("tomorrow"))), "Invalid expires_at") {
		t.Error("a date that is not RFC 3339 accepted")
	}
	unknown := plannedToken("ci", []string{"publish"}, nil, types.StringUnknown())
	unknown.Permissions = types.SetUnknown(types.StringType)
	assertEqual(t, "", check(unknown), "unknown values are checked at apply")
}

// UC: UC-IAC-015
func TestTheSameInstantWrittenAnotherWayIsNotAChange(t *testing.T) {
	plan := func(config, state types.String) types.String {
		resp := &planmodifier.StringResponse{PlanValue: config}
		sameInstantModifier{}.PlanModifyString(ctxb, planmodifier.StringRequest{ConfigValue: config, StateValue: state}, resp)
		return resp.PlanValue
	}
	server := types.StringValue("2027-01-01T00:00:00.000Z")
	assertEqual(t, server, plan(types.StringValue("2027-01-01T00:00:00Z"), server), "same instant: the state")
	assertEqual(t, types.StringValue("2027-01-02T00:00:00Z"), plan(types.StringValue("2027-01-02T00:00:00Z"), server), "another instant")
	assertEqual(t, types.StringNull(), plan(types.StringNull(), server), "removed: null")
	assertEqual(t, true, plan(types.StringUnknown(), server).IsUnknown(), "unknown")
}

// UC: UC-IAC-012
func TestTheTokensAreListedOldestFirst(t *testing.T) {
	newer := tokenJSON("t2", "ci-b", []string{"publish"}, nil, nil)
	newer["createdAt"] = "2026-11-07T11:00:00.000Z"
	f, c := newFakeServer(t, page(newer, tokenJSON("t1", "ci-a", []string{"consume"}, []string{"orders"}, nil)))
	m := tokensModel{NameContains: types.StringValue("ci")}
	if diags := listTokens(ctxb, c, &m); diags.HasError() {
		t.Fatal(diags)
	}
	assertEqual(t, "name=ci", f.all()[0].Query, "the filter goes to the server")
	assertEqual(t, `["t1","t2"]`, m.IDs.String(), "ids")
	assertEqual(t, 2, len(m.Tokens), "tokens")
}
