package taba

import "testing"

func tokenJSON(id string, extra ...map[string]interface{}) map[string]interface{} {
	data := map[string]interface{}{
		"id": id, "name": "deploy", "permissions": []string{"consume", "publish"},
		"topicPrefixes": []string{"orders."}, "createdAt": "2026-11-07T10:00:00Z", "expiresAt": nil,
	}
	for _, e := range extra {
		for k, v := range e {
			data[k] = v
		}
	}
	return data
}

// UC: UC-CNT-044
func TestTheTokenCallsBuildTheExpectedRoutes(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(200, tokenJSON("a b", map[string]interface{}{"token": "tb_x.y"})))

	f.client.CreateToken(ctxb, &TokenRequest{Name: "deploy", Permissions: []string{"publish"}})
	f.client.GetToken(ctxb, "a b")
	f.client.ListTokens(ctxb, Int(5), "cur", "dep")
	f.client.UpdateToken(ctxb, "a b", &TokenUpdate{Name: String("new")})
	f.client.RevokeToken(ctxb, "a b")

	got := f.server.all()
	want := []struct{ method, path string }{
		{"POST", "/tokens"},
		{"GET", "/tokens/a%20b"},
		{"GET", "/tokens"},
		{"PATCH", "/tokens/a%20b"},
		{"DELETE", "/tokens/a%20b"},
	}
	if len(got) != len(want) {
		t.Fatalf("want %d requests, got %d", len(want), len(got))
	}
	for i, w := range want {
		assertEqual(t, w.method, got[i].Method, "method")
		assertEqual(t, w.path, got[i].Path, "path")
	}
	assertEqual(t, "5", got[2].Query["limit"], "limit")
	assertEqual(t, "cur", got[2].Query["cursor"], "cursor")
	assertEqual(t, "dep", got[2].Query["name"], "name")
	assertEqual(t, `{"name":"new"}`, got[3].text(), "patch body")
}

// UC: UC-CNT-044
func TestCreateTokenSendsOnlyWhatWasFilledAndReturnsTheSecret(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(
		reply(201, tokenJSON("t1", map[string]interface{}{"token": "tb_t1.secret"})),
		reply(201, tokenJSON("t2", map[string]interface{}{"token": "tb_t2.secret", "topicPrefixes": []string{}})),
	)

	created, err := f.client.CreateToken(ctxb, &TokenRequest{
		Name: "deploy", Permissions: []string{"publish", "consume"},
		TopicPrefixes: []string{"orders."}, ExpiresAt: "2027-01-01T00:00:00Z",
	})
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "tb_t1.secret", created.Secret, "secret")
	assertEqual(t, "t1", created.ID, "id")
	assertEqual(t, 2, len(created.Permissions), "permissions")
	assertEqual(t, "orders.", created.TopicPrefixes[0], "prefix")
	assertEqual(t, "", created.ExpiresAt, "a null expiresAt is empty")

	_, err = f.client.CreateToken(ctxb, &TokenRequest{Name: "bare", Permissions: []string{"publish"}})
	if err != nil {
		t.Fatal(err)
	}
	got := f.server.all()
	assertEqual(t, `{"name":"deploy","permissions":["publish","consume"],"topicPrefixes":["orders."],"expiresAt":"2027-01-01T00:00:00Z"}`, got[0].text(), "full body")
	assertEqual(t, `{"name":"bare","permissions":["publish"]}`, got[1].text(), "only what was filled")
}

// UC: UC-CNT-044
func TestCreateTokenDoesNotRepeatA503WhoseOwnerMayHaveCreatedIt(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(reply(503, `{"code":"cluster.quorum-unavailable","message":"The definition's owner did not answer: x."}`))
	f.server.setFallback(reply(201, tokenJSON("t2", map[string]interface{}{"token": "tb_t2.s"})))

	_, err := f.client.CreateToken(ctxb, &TokenRequest{Name: "deploy", Permissions: []string{"publish"}})
	e := assertError(t, "cluster.quorum-unavailable", err)
	assertEqual(t, 503, e.Status, "status")
	assertEqual(t, 1, f.server.count(), "a single attempt: a second could create a second token")
}

// UC: UC-CNT-044
func TestCreateTokenRepeatsA503ThatDidNotReachTheOwner(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(reply(503, `{"code":"cluster.node-syncing","message":"x"}`))
	f.server.setFallback(reply(201, tokenJSON("t1", map[string]interface{}{"token": "tb_t1.s"})))

	created, err := f.client.CreateToken(ctxb, &TokenRequest{Name: "deploy", Permissions: []string{"publish"}})
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "tb_t1.s", created.Secret, "secret")
	assertEqual(t, 2, f.server.count(), "attempts")
}

// UC: UC-CNT-044
func TestACreationAnswerWithoutTheSecretIsAnInvalidResponse(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(201, tokenJSON("t1")))

	_, err := f.client.CreateToken(ctxb, &TokenRequest{Name: "deploy", Permissions: []string{"publish"}})
	assertError(t, "client.invalid-response", err)
}

// UC: UC-CNT-044
func TestUpdateTokenSendsANullDeadlineAndADate(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(200, tokenJSON("t1", map[string]interface{}{"expiresAt": "2027-01-01T00:00:00Z"})))

	token, err := f.client.UpdateToken(ctxb, "t1", &TokenUpdate{ExpiresAt: SetString("2027-01-01T00:00:00Z")})
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "2027-01-01T00:00:00Z", token.ExpiresAt, "expiresAt")
	f.client.UpdateToken(ctxb, "t1", &TokenUpdate{ExpiresAt: ClearString()})
	f.client.UpdateToken(ctxb, "t1", &TokenUpdate{})

	got := f.server.all()
	assertEqual(t, `{"expiresAt":"2027-01-01T00:00:00Z"}`, got[0].text(), "date")
	assertEqual(t, `{"expiresAt":null}`, got[1].text(), "null removes the deadline")
	assertEqual(t, `{}`, got[2].text(), "nothing filled")

	_, err = f.client.UpdateToken(ctxb, "t1", nil)
	assertError(t, "client.invalid-request", err)
}

// UC: UC-CNT-044
func TestListAllTokensReadsEveryPage(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(
		reply(200, map[string]interface{}{"items": []interface{}{tokenJSON("t1")}, "nextCursor": "c2"}),
		reply(200, map[string]interface{}{"items": []interface{}{tokenJSON("t2")}}),
	)

	all, err := f.client.ListAllTokens(ctxb, "dep")
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, 2, len(all), "tokens")
	assertEqual(t, "t2", all[1].ID, "second page")
	got := f.server.all()
	assertEqual(t, "c2", got[1].Query["cursor"], "cursor")
	assertEqual(t, "dep", got[1].Query["name"], "name on every page")
}

// UC: UC-CNT-044
func TestAnExpiredOrRevokedTokenIsNotFoundAndARevokeAfterA503Succeeds(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	notFound := reply(404, `{"code":"token.not-found","message":"x"}`)
	f.server.reply(
		notFound,
		reply(503, `{"code":"cluster.quorum-unavailable","message":"x"}`),
		notFound,
		notFound,
	)

	_, err := f.client.GetToken(ctxb, "t1")
	assertError(t, "token.not-found", err)
	if err := f.client.RevokeToken(ctxb, "t1"); err != nil {
		t.Fatalf("a 404 after a 503 is the revocation already applied: %v", err)
	}
	err = f.client.RevokeToken(ctxb, "t1")
	assertError(t, "token.not-found", err)
}
