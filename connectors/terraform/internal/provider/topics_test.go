package provider

import (
	"strings"
	"testing"

	"github.com/hashicorp/terraform-plugin-framework/attr"
	"github.com/hashicorp/terraform-plugin-framework/types"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

// declaredQueue is a queue model as the plan gives it: the defaults of the schema filled in.
func declaredQueue(name string) queueModel {
	return queueModel{
		ID: types.StringUnknown(), Name: types.StringValue(name),
		Fifo: types.BoolValue(false), Volatile: types.BoolValue(false),
		Tags: types.MapValueMust(types.StringType, map[string]attr.Value{}),
		TTL:  types.Int64Null(), MaxDeliveries: types.Int64Null(), MaxMessageBytes: types.Int64Null(),
		IdleTimeout: types.Int64Null(), ForceDestroy: types.BoolValue(false),
		CreatedAt: types.StringUnknown(), DeadLetterName: types.StringValue(name + deadLetterSuffix),
		EffectiveTTL: types.Int64Unknown(), EffectiveMaxDeliveries: types.Int64Unknown(), EffectiveMaxMessageBytes: types.Int64Unknown(),
	}
}

// UC: UC-IAC-002
func TestTheQueueBodyCarriesOnlyWhatWasDeclared(t *testing.T) {
	m := declaredQueue("orders")
	assertJSON(t, `{"name":"orders","type":"queue"}`, bodyOf(t, m), "nothing declared: the node defaults")

	m.Fifo = types.BoolValue(true)
	m.Tags = mapValue(map[string]string{"team": "billing"})
	m.TTL = types.Int64Value(3600)
	m.MaxDeliveries = types.Int64Value(5)
	m.MaxMessageBytes = types.Int64Value(1024)
	assertJSON(t, `{"name":"orders","type":"queue","fifo":true,"tags":{"team":"billing"},"ttl":3600,"maxDeliveries":5,"maxMessageBytes":1024}`,
		bodyOf(t, m), "everything declared")

	v := declaredQueue("temp")
	v.Volatile = types.BoolValue(true)
	v.IdleTimeout = types.Int64Value(900)
	assertJSON(t, `{"name":"temp","type":"queue","volatile":true,"idleTimeout":900}`, bodyOf(t, v), "volatile")
}

// bodyOf is the body createQueue sends for m.
func bodyOf(t *testing.T, m queueModel) string {
	t.Helper()
	f, c := newFakeServer(t, ok(201, queueJSON(m.Name.ValueString(), nil)))
	createQueue(ctxb, c, m, false)
	return f.all()[0].Body
}

// UC: UC-IAC-002
func TestCreatingAQueueFillsTheStateFromTheServer(t *testing.T) {
	f, c := newFakeServer(t, ok(201, queueJSON("orders", map[string]any{"ttl": 3600, "effective": map[string]int{"ttl": 3600, "maxDeliveries": 10, "maxMessageBytes": 5242880}})))
	m := declaredQueue("orders")
	m.TTL = types.Int64Value(3600)
	got, diags := createQueue(ctxb, c, m, false)
	if diags.HasError() {
		t.Fatal(diags)
	}
	r := f.all()[0]
	assertEqual(t, "POST /topics", r.Method+" "+r.Path, "route")
	assertEqual(t, "id-orders", got.ID.ValueString(), "id")
	assertEqual(t, "2026-11-07T10:00:00Z", got.CreatedAt.ValueString(), "created_at")
	assertEqual(t, "orders-dead-letter", got.DeadLetterName.ValueString(), "dead_letter_name")
	assertEqual(t, int64(3600), got.EffectiveTTL.ValueInt64(), "effective_ttl")
	assertEqual(t, int64(10), got.EffectiveMaxDeliveries.ValueInt64(), "effective_max_deliveries")
	assertEqual(t, true, got.MaxDeliveries.IsNull(), "max_deliveries follows the node default")
	assertEqual(t, false, got.ForceDestroy.ValueBool(), "force_destroy kept")
}

// UC: UC-IAC-009
func TestAQueueThatAlreadyExistsIsAnErrorThatPointsToImport(t *testing.T) {
	_, c := newFakeServer(t, failure(409, "topic.already-exists"))
	_, diags := createQueue(ctxb, c, declaredQueue("orders"), false)
	if !diags.HasError() {
		t.Fatal("no error")
	}
	if !strings.Contains(diags.Errors()[0].Detail(), "terraform import") {
		t.Errorf("no import hint: %s", diags.Errors()[0].Detail())
	}
}

// UC: UC-IAC-010
func TestACreationInterruptedByA503IsOursOnlyIfTheTopicIsWhatWasDeclared(t *testing.T) {
	// 503, then 409 on the retry: the connector reads the topic, which the creation left as declared.
	f, c := newFakeServer(t, unavailable("cluster.commit-timeout"), failure(409, "topic.already-exists"), ok(200, queueJSON("orders", nil)))
	got, diags := createQueue(ctxb, c, declaredQueue("orders"), false)
	if diags.HasError() {
		t.Fatal(diags)
	}
	assertEqual(t, "id-orders", got.ID.ValueString(), "adopted the topic of the creation")
	assertEqual(t, 3, len(f.all()), "POST, POST, GET")

	// The same, but the topic has another ttl: someone else created it at the same time.
	_, c = newFakeServer(t, unavailable("cluster.commit-timeout"), failure(409, "topic.already-exists"), ok(200, queueJSON("orders", map[string]any{"ttl": 60})))
	_, diags = createQueue(ctxb, c, declaredQueue("orders"), false)
	if !diags.HasError() || !strings.Contains(diags.Errors()[0].Detail(), "ttl") {
		t.Fatalf("a different topic was adopted: %v", diags)
	}

	// Or it is a distributor.
	_, c = newFakeServer(t, unavailable("cluster.commit-timeout"), failure(409, "topic.already-exists"), ok(200, distributorJSON("orders", []string{"a"})))
	_, diags = createQueue(ctxb, c, declaredQueue("orders"), false)
	if !diags.HasError() {
		t.Fatal("a distributor was adopted as a queue")
	}
}

// UC: UC-IAC-006
func TestReadingAQueueThatIsGoneRemovesItFromTheState(t *testing.T) {
	_, c := newFakeServer(t, failure(404, "topic.not-found"))
	_, found, diags := readQueue(ctxb, c, declaredQueue("orders"))
	if diags.HasError() || found {
		t.Fatalf("found=%v diags=%v", found, diags)
	}
}

// UC: UC-IAC-007
func TestReadingADistributorOrADeadLetterAsAQueueIsAnError(t *testing.T) {
	for name, topic := range map[string]map[string]any{
		"distributor": distributorJSON("fanout", []string{"a"}),
		"dead-letter": queueJSON("orders-dead-letter", map[string]any{"deadLetter": true, "source": "orders"}),
	} {
		_, c := newFakeServer(t, ok(200, topic))
		_, found, diags := readQueue(ctxb, c, declaredQueue("x"))
		if !diags.HasError() || found {
			t.Errorf("%s: found=%v diags=%v", name, found, diags)
		}
	}
}

// UC: UC-IAC-007
func TestReadingAQueueKeepsForceDestroyAndAnImportStartsWithFalse(t *testing.T) {
	_, c := newFakeServer(t, ok(200, queueJSON("orders", map[string]any{"tags": map[string]string{"a": "b"}})))
	state := declaredQueue("orders")
	state.ForceDestroy = types.BoolValue(true)
	got, _, _ := readQueue(ctxb, c, state)
	assertEqual(t, true, got.ForceDestroy.ValueBool(), "kept")
	assertEqual(t, "b", stringMap(got.Tags)["a"], "tags")

	imported := queueModel{Name: types.StringValue("orders"), ForceDestroy: types.BoolNull()}
	got, _, _ = readQueue(ctxb, c, imported)
	assertEqual(t, types.BoolValue(false), got.ForceDestroy, "after an import")
}

// UC: UC-IAC-004
func TestUpdatingAQueueSendsTheWholeDeclaredStateByPut(t *testing.T) {
	f, c := newFakeServer(t, ok(200, queueJSON("orders", nil)))
	m := declaredQueue("orders")
	m.MaxDeliveries = types.Int64Value(3)
	_, diags := updateQueue(ctxb, c, m)
	if diags.HasError() || diags.WarningsCount() != 0 {
		t.Fatal(diags)
	}
	r := f.all()[0]
	assertEqual(t, "PUT /topics/orders", r.Method+" "+r.Path, "route")
	assertEqual(t, "", r.Query, "no recreate")
	assertJSON(t, `{"type":"queue","maxDeliveries":3}`, r.Body, "what is left out goes back to the default")
}

// UC: UC-IAC-004
func TestUpdatingAQueueSomeoneRemovedWarnsThatItWasCreatedAgain(t *testing.T) {
	_, c := newFakeServer(t, ok(201, queueJSON("orders", nil)))
	_, diags := updateQueue(ctxb, c, declaredQueue("orders"))
	if diags.HasError() || diags.WarningsCount() != 1 {
		t.Fatalf("want one warning: %v", diags)
	}
}

// UC: UC-IAC-008
func TestRemovingAQueueOnlyIfEmptyUnlessForced(t *testing.T) {
	f, c := newFakeServer(t, answer{status: 204})
	if d := deleteTopic(ctxb, c, "queue", "orders", false); d.HasError() {
		t.Fatal(d)
	}
	r := f.all()[0]
	assertEqual(t, "DELETE /topics/orders", r.Method+" "+r.Path, "route")
	assertEqual(t, "ifEmpty=true", r.Query, "only if empty")

	f, c = newFakeServer(t, answer{status: 204})
	if d := deleteTopic(ctxb, c, "queue", "orders", true); d.HasError() {
		t.Fatal(d)
	}
	assertEqual(t, "", f.all()[0].Query, "force_destroy: a plain DELETE")
}

// UC: UC-IAC-008
func TestAQueueWithMessagesIsNotRemovedAndTheErrorSaysHowToProceed(t *testing.T) {
	_, c := newFakeServer(t, answer{status: 409, body: `{"code":"topic.not-empty","message":"x","details":[{"field":"messages","code":"topic.not-empty","message":"The queue and its dead-letter hold 3 messages; nothing was changed."}]}`})
	d := deleteTopic(ctxb, c, "queue", "orders", false)
	if !d.HasError() {
		t.Fatal("removed a queue with messages")
	}
	detail := d.Errors()[0].Detail()
	for _, want := range []string{"force_destroy", "- messages: The queue and its dead-letter hold 3 messages"} {
		if !strings.Contains(detail, want) {
			t.Errorf("the error lacks %q: %s", want, detail)
		}
	}
}

// UC: UC-IAC-008
func TestRemovingATopicThatIsAlreadyGoneSucceeds(t *testing.T) {
	_, c := newFakeServer(t, failure(404, "topic.not-found"))
	if d := deleteTopic(ctxb, c, "queue", "orders", false); d.HasError() {
		t.Fatal(d)
	}
}

func declaredDistributor(name string, prefixes ...string) distributorModel {
	return distributorModel{
		ID: types.StringUnknown(), Name: types.StringValue(name), Prefixes: listValue(prefixes),
		Tags: types.MapValueMust(types.StringType, map[string]attr.Value{}), CreatedAt: types.StringUnknown(),
	}
}

// UC: UC-IAC-011
func TestADistributorIsCreatedAndUpdatedWithItsPrefixesInOrder(t *testing.T) {
	f, c := newFakeServer(t, ok(201, distributorJSON("fanout", []string{"orders-", "audit-"})), ok(200, distributorJSON("fanout", []string{"audit-"})))
	got, diags := createDistributor(ctxb, c, declaredDistributor("fanout", "orders-", "audit-"), false)
	if diags.HasError() {
		t.Fatal(diags)
	}
	assertEqual(t, "orders-,audit-", strings.Join(stringList(got.Prefixes), ","), "prefixes in order")
	_, diags = updateDistributor(ctxb, c, declaredDistributor("fanout", "audit-"))
	if diags.HasError() {
		t.Fatal(diags)
	}
	all := f.all()
	assertJSON(t, `{"name":"fanout","type":"distributor","prefixes":["orders-","audit-"]}`, all[0].Body, "POST")
	assertEqual(t, "PUT /topics/fanout", all[1].Method+" "+all[1].Path, "update route")
	assertJSON(t, `{"type":"distributor","prefixes":["audit-"]}`, all[1].Body, "PUT")
}

// UC: UC-IAC-012
func TestListingTopicsFiltersByTypeTagsAndDeadLetter(t *testing.T) {
	f, c := newFakeServer(t, ok(200, map[string]any{"items": []any{
		queueJSON("orders", map[string]any{"tags": map[string]string{"team": "billing"}}),
		queueJSON("orders-dead-letter", map[string]any{"deadLetter": true, "source": "orders", "tags": map[string]string{"team": "billing"}}),
		distributorJSON("orders-fanout", []string{"orders"}),
		queueJSON("audit", nil),
	}}))
	m := topicsModel{
		NameContains: types.StringValue("orders"), Type: types.StringNull(),
		Tags: types.MapNull(types.StringType), IncludeDeadLetters: types.BoolNull(),
	}
	if d := listTopics(ctxb, c, &m); d.HasError() {
		t.Fatal(d)
	}
	assertEqual(t, "name=orders", f.all()[0].Query, "the name filter goes to the server")
	assertEqual(t, "audit,orders,orders-fanout", strings.Join(stringList(m.Names), ","), "sorted, no dead-letter")
	assertEqual(t, "orders", strings.Join(stringList(m.Topics[2].Prefixes), ","), "distributor prefixes")
	assertEqual(t, true, m.Topics[0].Prefixes.IsNull(), "a queue has no prefixes")

	m.Type = types.StringValue("queue")
	m.Tags = mapValue(map[string]string{"team": "billing"})
	m.IncludeDeadLetters = types.BoolValue(true)
	if d := listTopics(ctxb, c, &m); d.HasError() {
		t.Fatal(d)
	}
	assertEqual(t, "orders,orders-dead-letter", strings.Join(stringList(m.Names), ","), "queues with the tag, dead-letter included")
}

// UC: UC-IAC-023
func TestTheErrorDetailsAreShownAsLines(t *testing.T) {
	list := &taba.Error{Code: "validation.failed", Details: `[{"field":"ttl","code":"validation.out-of-range","message":"60 to 31536000"},{"message":"other"}]`}
	assertEqual(t, "- ttl: 60 to 31536000\n- other", detailsText(list), "a list of details")
	assertEqual(t, `Details: {"a":1}`, detailsText(&taba.Error{Details: `{"a":1}`}), "anything else as it came")
	assertEqual(t, "", detailsText(&taba.Error{Details: "null"}), "no details")
}
