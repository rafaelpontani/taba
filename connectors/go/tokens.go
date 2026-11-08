package taba

import (
	"context"
	"strconv"
)

// The access tokens of the client API (doc 06a, "Access tokens"): POST and GET /tokens and GET, PATCH and DELETE
// /tokens/{id}. They only exist with authentication on (otherwise 404 route.not-found) and only the root token and
// the admin tokens use them; an admin token itself is only created, changed and revoked by the root.

// quorumUnavailable is the 503 of a definition whose owner did not answer: the change may have been applied.
const quorumUnavailable = "cluster.quorum-unavailable"

// Token is a token of the API. It never carries the secret.
type Token struct {
	ID   string
	Name string
	// Permissions as the server keeps them: sorted and without repetition (publish, consume, create, manage, admin).
	Permissions []string
	// TopicPrefixes limit the token to topics whose name starts with one of them; empty reaches every topic.
	TopicPrefixes []string
	// CreatedAt is the creation date, as RFC 3339 text.
	CreatedAt string
	// ExpiresAt is the deadline as RFC 3339 text, or "" when the token does not expire.
	ExpiresAt string
	// Raw is the token JSON, as the server returned it.
	Raw string
}

// TokenCreated is the answer of CreateToken: the token and its secret, which the server shows only here.
type TokenCreated struct {
	Token
	// Secret is the whole token to put in Authorization: Bearer (tb_<id>.<secret>).
	Secret string
}

// TokenPage is one page of ListTokens.
type TokenPage struct {
	Items []Token
	// NextCursor is the cursor of the next page, or "" on the last one.
	NextCursor string
	Raw        string
}

// TokenRequest is POST /tokens. Name and Permissions are required; an empty TopicPrefixes and ExpiresAt are left
// out of the request.
type TokenRequest struct {
	Name        string
	Permissions []string
	// TopicPrefixes cannot go with the admin permission (400).
	TopicPrefixes []string
	// ExpiresAt is an RFC 3339 date in the future; "" means the token does not expire.
	ExpiresAt string
}

// TokenUpdate is PATCH /tokens/{id}: only the name and the deadline change (permissions and prefixes are immutable:
// create another token and revoke this one). Only what was filled in is sent.
type TokenUpdate struct {
	Name *string
	// ExpiresAt: nil leaves it out, ClearString() sends null (no deadline) and SetString(date) sends the date.
	ExpiresAt *NullableString
}

// NullableString is a field that can also be sent as null. Use SetString or ClearString; a nil *NullableString
// leaves the field out of the request.
type NullableString struct {
	value string
	null  bool
}

// SetString sends s.
func SetString(s string) *NullableString { return &NullableString{value: s} }

// ClearString sends null.
func ClearString() *NullableString { return &NullableString{null: true} }

// -- the calls ------------------------------------------------------------------------------------------------

func createTokenCall(r *TokenRequest) (*apiCall, error) {
	if r == nil {
		return nil, clientError("client.invalid-request", "request is required")
	}
	var o jsonObject
	o.put("name", r.Name)
	permissions := r.Permissions
	if permissions == nil {
		permissions = []string{}
	}
	o.put("permissions", permissions)
	if len(r.TopicPrefixes) > 0 {
		o.put("topicPrefixes", r.TopicPrefixes)
	}
	if r.ExpiresAt != "" {
		o.put("expiresAt", r.ExpiresAt)
	}
	call := (&apiCall{method: "POST", path: "/tokens"}).withJSON(o.bytes())
	call.noRetry = []string{quorumUnavailable}
	return call, nil
}

func getTokenCall(id string) *apiCall {
	return &apiCall{method: "GET", path: "/tokens/" + segment(id)}
}

func listTokensCall(limit *int, cursor, name string) *apiCall {
	call := &apiCall{method: "GET", path: "/tokens"}
	if limit != nil {
		call.withQuery("limit", strconv.Itoa(*limit))
	}
	if cursor != "" {
		call.withQuery("cursor", cursor)
	}
	if name != "" {
		call.withQuery("name", name)
	}
	return call
}

func updateTokenCall(id string, u *TokenUpdate) (*apiCall, error) {
	if u == nil {
		return nil, clientError("client.invalid-request", "update is required")
	}
	var o jsonObject
	if u.Name != nil {
		o.put("name", *u.Name)
	}
	if u.ExpiresAt != nil {
		if u.ExpiresAt.null {
			o.put("expiresAt", nil)
		} else {
			o.put("expiresAt", u.ExpiresAt.value)
		}
	}
	return (&apiCall{method: "PATCH", path: "/tokens/" + segment(id)}).withJSON(o.bytes()), nil
}

func revokeTokenCall(id string) *apiCall {
	call := &apiCall{method: "DELETE", path: "/tokens/" + segment(id)}
	call.recover = append(call.recover, recoverRule{404, "token.not-found"})
	return call
}

// -- the client -----------------------------------------------------------------------------------------------

// CreateToken is POST /tokens. The secret comes only in this answer (TokenCreated.Secret).
//
// A 503 cluster.quorum-unavailable is returned at once, without a new attempt: the owner of the definition may
// have created the token and the answer was lost, so a repetition could create a second one. In that case the
// token may exist without its secret having arrived; look for it (ListAllTokens by name, created after the call
// started) and revoke it before trying again. Other 503s and 429 are repeated as in the other calls.
func (c *Client) CreateToken(ctx context.Context, request *TokenRequest) (*TokenCreated, error) {
	call, err := createTokenCall(request)
	if err != nil {
		return nil, err
	}
	reply, err := c.t.send(ctx, call)
	if err != nil {
		return nil, err
	}
	return parseTokenCreated(reply.body)
}

// GetToken is GET /tokens/{id}. An expired token is 404 token.not-found, like a revoked one.
func (c *Client) GetToken(ctx context.Context, id string) (*Token, error) {
	reply, err := c.t.send(ctx, getTokenCall(id))
	if err != nil {
		return nil, err
	}
	t, err := parseToken(reply.body)
	if err != nil {
		return nil, err
	}
	return &t, nil
}

// ListTokens is GET /tokens. name keeps only the tokens whose name contains the text. A nil limit and empty
// cursor and name do not go in the query. Expired tokens are not listed.
func (c *Client) ListTokens(ctx context.Context, limit *int, cursor, name string) (*TokenPage, error) {
	reply, err := c.t.send(ctx, listTokensCall(limit, cursor, name))
	if err != nil {
		return nil, err
	}
	return parseTokenPage(reply.body)
}

// ListAllTokens returns all tokens (whose name contains name, when not empty), reading the pages one after the other.
func (c *Client) ListAllTokens(ctx context.Context, name string) ([]Token, error) {
	all := []Token{}
	cursor := ""
	for {
		page, err := c.ListTokens(ctx, nil, cursor, name)
		if err != nil {
			return nil, err
		}
		all = append(all, page.Items...)
		cursor = page.NextCursor
		if cursor == "" {
			return all, nil
		}
	}
}

// UpdateToken is PATCH /tokens/{id}: the name and the deadline.
func (c *Client) UpdateToken(ctx context.Context, id string, update *TokenUpdate) (*Token, error) {
	call, err := updateTokenCall(id, update)
	if err != nil {
		return nil, err
	}
	reply, err := c.t.send(ctx, call)
	if err != nil {
		return nil, err
	}
	t, err := parseToken(reply.body)
	if err != nil {
		return nil, err
	}
	return &t, nil
}

// RevokeToken is DELETE /tokens/{id}. If a 503 hid the answer and the token is gone on the retry, it succeeds.
func (c *Client) RevokeToken(ctx context.Context, id string) error {
	_, err := c.t.send(ctx, revokeTokenCall(id))
	return err
}

// -- reading the responses ------------------------------------------------------------------------------------

func tokenFrom(f jsonFields, raw []byte) (Token, error) {
	t := Token{Raw: string(raw)}
	var errs [6]error
	t.ID, _, errs[0] = f.str("id")
	t.Name, _, errs[1] = f.str("name")
	t.Permissions, errs[2] = f.strings("permissions")
	t.TopicPrefixes, errs[3] = f.strings("topicPrefixes")
	t.CreatedAt, _, errs[4] = f.str("createdAt")
	t.ExpiresAt, _, errs[5] = f.str("expiresAt")
	if err := firstError(errs[:]...); err != nil {
		return Token{}, invalid("token", err)
	}
	return t, nil
}

func parseToken(raw []byte) (Token, error) {
	f, err := parseFields(raw)
	if err != nil {
		return Token{}, invalid("token", err)
	}
	return tokenFrom(f, raw)
}

func parseTokenCreated(raw []byte) (*TokenCreated, error) {
	f, err := parseFields(raw)
	if err != nil {
		return nil, invalid("token", err)
	}
	t, err := tokenFrom(f, raw)
	if err != nil {
		return nil, err
	}
	secret, ok, err := f.str("token")
	if err != nil || !ok {
		return nil, clientError("client.invalid-response", "token: the creation answer has no secret")
	}
	return &TokenCreated{Token: t, Secret: secret}, nil
}

func parseTokenPage(raw []byte) (*TokenPage, error) {
	f, err := parseFields(raw)
	if err != nil {
		return nil, invalid("token page", err)
	}
	page := &TokenPage{Raw: string(raw), Items: []Token{}}
	if listing := f.raw("items"); listing != nil {
		items, err := arrayItems(listing)
		if err != nil {
			return nil, invalid("token page items", err)
		}
		for _, item := range items {
			t, err := parseToken(item)
			if err != nil {
				return nil, err
			}
			page.Items = append(page.Items, t)
		}
	}
	page.NextCursor, _, err = f.str("nextCursor")
	if err != nil {
		return nil, invalid("token page", err)
	}
	return page, nil
}
