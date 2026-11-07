package provider

import (
	"context"
	"fmt"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/hashicorp/terraform-plugin-framework-validators/setvalidator"
	"github.com/hashicorp/terraform-plugin-framework-validators/stringvalidator"
	"github.com/hashicorp/terraform-plugin-framework/attr"
	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/setdefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/setplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/schema/validator"
	"github.com/hashicorp/terraform-plugin-framework/types"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

// tokenNamePattern is the name of a token (doc 16): like a tag value, 1 to 100 characters. It is not unique.
var tokenNamePattern = regexp.MustCompile(`^[A-Za-z0-9_.:-]{1,100}$`)

// permissions are the permissions of a token (doc 14).
var permissions = []string{"publish", "consume", "create", "manage", "admin"}

// serverTime is the only date format the server accepts and returns: RFC 3339 in UTC with milliseconds.
const serverTime = "2006-01-02T15:04:05.000Z"

var (
	_ resource.Resource                   = (*tokenResource)(nil)
	_ resource.ResourceWithConfigure      = (*tokenResource)(nil)
	_ resource.ResourceWithImportState    = (*tokenResource)(nil)
	_ resource.ResourceWithValidateConfig = (*tokenResource)(nil)
)

func newTokenResource() resource.Resource { return &tokenResource{now: time.Now} }

type tokenResource struct {
	client         *taba.Client
	createOrUpdate bool
	now            func() time.Time
}

type tokenModel struct {
	ID             types.String `tfsdk:"id"`
	Name           types.String `tfsdk:"name"`
	Permissions    types.Set    `tfsdk:"permissions"`
	TopicPrefixes  types.Set    `tfsdk:"topic_prefixes"`
	ExpiresAt      types.String `tfsdk:"expires_at"`
	CreatedAt      types.String `tfsdk:"created_at"`
	Token          types.String `tfsdk:"token"`
	CreateOrUpdate types.Bool   `tfsdk:"create_or_update"`
}

func (r *tokenResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_token"
}

func (r *tokenResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "An access token of the client API. Tokens only exist with authentication on, and the provider token must be the root " +
			"token or an `admin` token (a token with `admin` is only created by the root token).\n\n" +
			"The secret (`token`) only exists in the creation answer, so it is kept in the state: treat the state as a secret. " +
			"Changing `permissions` or `topic_prefixes` replaces the token (a new secret); `name` and `expires_at` change in place. " +
			"To rotate, `terraform apply -replace=<address>`, with `lifecycle { create_before_destroy = true }` so there is no moment without a token.",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				MarkdownDescription: "Identifier of the token (the part after `tb_` in the secret).",
				Computed:            true,
				PlanModifiers:       []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
			"name": schema.StringAttribute{
				MarkdownDescription: "Name: 1 to 100 characters `A-Za-z0-9_-.:`. It is not unique on the server. Changes in place.",
				Required:            true,
				Validators:          []validator.String{stringvalidator.RegexMatches(tokenNamePattern, "must have 1 to 100 characters A-Za-z0-9_-.:")},
			},
			"permissions": schema.SetAttribute{
				MarkdownDescription: "Permissions: one or more of `publish`, `consume`, `create`, `manage`, `admin`. Changing them replaces the token.",
				ElementType:         types.StringType,
				Required:            true,
				Validators: []validator.Set{
					setvalidator.SizeAtLeast(1),
					setvalidator.ValueStringsAre(stringvalidator.OneOf(permissions...)),
				},
				PlanModifiers: []planmodifier.Set{setplanmodifier.RequiresReplace()},
			},
			"topic_prefixes": schema.SetAttribute{
				MarkdownDescription: "Limit the token to the topics whose name starts with one of these prefixes (up to 20; 1 to 100 characters `A-Za-z0-9_-`). Empty: every topic. Not allowed with `admin`. Changing them replaces the token. Default: none.",
				ElementType:         types.StringType,
				Optional:            true,
				Computed:            true,
				Default:             setdefault.StaticValue(types.SetValueMust(types.StringType, []attr.Value{})),
				Validators: []validator.Set{
					setvalidator.SizeAtMost(20),
					setvalidator.ValueStringsAre(stringvalidator.RegexMatches(prefixPattern, "must have 1 to 100 characters A-Za-z0-9_-")),
				},
				PlanModifiers: []planmodifier.Set{setplanmodifier.RequiresReplace()},
			},
			"expires_at": schema.StringAttribute{
				MarkdownDescription: "Deadline (RFC 3339, such as `2027-01-01T00:00:00Z`); it must be in the future. Not set: the token does not expire. " +
					"Changes in place. An expired token disappears from the server: the next `plan` then refuses the configuration until `expires_at` " +
					"moves to the future (a new token) or the resource is removed.",
				Optional:      true,
				Computed:      true,
				PlanModifiers: []planmodifier.String{sameInstantModifier{}},
			},
			"created_at": schema.StringAttribute{
				MarkdownDescription: "Creation date (RFC 3339).",
				Computed:            true,
				PlanModifiers:       []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
			"token": schema.StringAttribute{
				MarkdownDescription: "The secret to send in `Authorization: Bearer` (`tb_<id>.<secret>`). Only known when the token was created here: null after an import.",
				Computed:            true,
				Sensitive:           true,
				PlanModifiers:       []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
			"create_or_update": createOrUpdateAttribute("When creating, take over the token with this `name` that already exists instead of creating another (names are not unique): it must be the only one with the name and have the declared `permissions` and `topic_prefixes`, and its `expires_at` changes to the declared one. Its secret cannot be recovered: `token` stays null (use `-replace` for a new one)."),
		},
	}
}

func (r *tokenResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	r.client, r.createOrUpdate = configureResource(req, resp)
}

func (r *tokenResource) ValidateConfig(ctx context.Context, req resource.ValidateConfigRequest, resp *resource.ValidateConfigResponse) {
	var m tokenModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(validateToken(m, r.now())...)
}

// validateToken is what the schema cannot say alone: admin without prefixes and a deadline in the future.
func validateToken(m tokenModel, now time.Time) diag.Diagnostics {
	var diags diag.Diagnostics
	if !m.Permissions.IsUnknown() && !m.TopicPrefixes.IsUnknown() && len(stringSet(m.TopicPrefixes)) > 0 {
		for _, p := range stringSet(m.Permissions) {
			if p == "admin" {
				diags.AddAttributeError(path.Root("topic_prefixes"), "admin reaches every topic",
					"A token with admin reaches every topic, so topic_prefixes would suggest a limit it does not have: remove topic_prefixes, or use manage to limit the token to prefixes.")
			}
		}
	}
	if m.ExpiresAt.IsNull() || m.ExpiresAt.IsUnknown() {
		return diags
	}
	at, err := time.Parse(time.RFC3339, m.ExpiresAt.ValueString())
	if err != nil {
		diags.AddAttributeError(path.Root("expires_at"), "Invalid expires_at",
			fmt.Sprintf("%q is not an RFC 3339 date (such as 2027-01-01T00:00:00Z).", m.ExpiresAt.ValueString()))
		return diags
	}
	if !at.After(now) {
		diags.AddAttributeError(path.Root("expires_at"), "The token has expired",
			fmt.Sprintf("expires_at (%s) is not in the future. An expired token no longer exists on the server: move expires_at to the future (a new token is created) or remove the resource.", m.ExpiresAt.ValueString()))
	}
	return diags
}

func (r *tokenResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan tokenModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	created, diags := createToken(ctx, r.client, plan, wantsCreateOrUpdate(plan.CreateOrUpdate, r.createOrUpdate))
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, created)...)
}

func (r *tokenResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state tokenModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	read, found, diags := readToken(ctx, r.client, state)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	if !found {
		resp.State.RemoveResource(ctx)
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, read)...)
}

func (r *tokenResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan, state tokenModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	updated, diags := updateToken(ctx, r.client, plan, state)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, updated)...)
}

func (r *tokenResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state tokenModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(revokeToken(ctx, r.client, state.ID.ValueString())...)
}

// ImportState takes the token id: terraform import taba_token.ci <id>. The secret does not come back (token is null).
func (r *tokenResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("id"), req, resp)
}

// -- the calls ------------------------------------------------------------------------------------------------------

// tokenError adds the diagnostic of a failed call to /tokens, with the hint for the two mistakes of configuration:
// authentication off (the routes do not exist) and a provider token that cannot manage tokens.
func tokenError(diags *diag.Diagnostics, summary string, err error) {
	switch errorCode(err) {
	case "route.not-found":
		diags.AddError(summary+": the node has no tokens",
			"The node answered 404 route.not-found on /tokens: tokens only exist with authentication on. Turn authentication on in the cluster (a root token), or remove the taba_token resources.\n"+err.Error())
	case "auth.forbidden":
		diags.AddError(summary+": the provider token cannot manage tokens",
			"Tokens are managed by the root token or a token with admin, and a token with admin only by the root token. Configure the provider with one of them (token or TABA_TOKEN).\n"+err.Error())
	case "auth.unauthorized":
		diags.AddError(summary+": the provider token was refused",
			"The node did not accept the provider token (missing, revoked or expired). Check token or TABA_TOKEN.\n"+err.Error())
	default:
		apiError(diags, summary, err)
	}
}

func tokenRequest(m tokenModel) *taba.TokenRequest {
	return &taba.TokenRequest{
		Name:          m.Name.ValueString(),
		Permissions:   stringSet(m.Permissions),
		TopicPrefixes: stringSet(m.TopicPrefixes),
		ExpiresAt:     serverDate(m.ExpiresAt),
	}
}

// tokenFromAPI is the state of the token as the server returned it. The secret and the text of expires_at come
// from prior: the server never returns the secret, and it writes the date in its own format, so the declared text
// is kept while it is the same instant.
func tokenFromAPI(t *taba.Token, prior tokenModel) tokenModel {
	m := tokenModel{
		ID:             types.StringValue(t.ID),
		Name:           types.StringValue(t.Name),
		Permissions:    setValue(t.Permissions),
		TopicPrefixes:  setValue(t.TopicPrefixes),
		ExpiresAt:      stringOrNull(t.ExpiresAt),
		CreatedAt:      stringOrNull(t.CreatedAt),
		Token:          prior.Token,
		CreateOrUpdate: knownOrNull(prior.CreateOrUpdate),
	}
	if m.Token.IsUnknown() {
		m.Token = types.StringNull()
	}
	if !prior.ExpiresAt.IsNull() && !prior.ExpiresAt.IsUnknown() && sameInstant(prior.ExpiresAt.ValueString(), t.ExpiresAt) {
		m.ExpiresAt = prior.ExpiresAt
	}
	return m
}

// tokenMatches says whether a listed token is the one declared: same name, permissions and prefixes.
func tokenMatches(t taba.Token, m tokenModel) bool {
	return t.Name == m.Name.ValueString() &&
		sameStrings(t.Permissions, stringSet(m.Permissions)) &&
		sameStrings(t.TopicPrefixes, stringSet(m.TopicPrefixes))
}

// createToken is POST /tokens; with create_or_update, the only token with the declared name is taken over instead
// (adoptToken). A 503 cluster.quorum-unavailable is the only answer the connector does not repeat:
// the owner of the definition may have created the token and the answer was lost, and the secret with it. Then
// the token that appeared since the call (same name, permissions and prefixes, not in the list taken before it)
// is revoked and the creation is tried once more. More than one such token means someone else is creating the
// same one: nothing is revoked, and the error names them.
func createToken(ctx context.Context, c *taba.Client, plan tokenModel, createOrUpdate bool) (tokenModel, diag.Diagnostics) {
	var diags diag.Diagnostics
	name := plan.Name.ValueString()
	summary := fmt.Sprintf("Creating the token %q", name)
	before, err := c.ListAllTokens(ctx, name)
	if err != nil {
		tokenError(&diags, summary, err)
		return plan, diags
	}
	if createOrUpdate {
		if existing, found := sameName(before, name, plan, &diags); found {
			return adoptToken(ctx, c, plan, existing)
		}
		if diags.HasError() {
			return plan, diags
		}
	}
	known := map[string]bool{}
	for _, t := range before {
		known[t.ID] = true
	}
	// The POST repeated after 503 cluster.quorum-unavailable, which the connector does not repeat (a lost answer may
	// have created the token), within the same retry_max_wait as every other call: a node that crashed is a member,
	// and the owner of some definitions, until the others vote it out (about a minute). Each attempt first revokes
	// what the previous one may have left.
	budget := c.Config().RetryMaxWait
	var waited time.Duration
	for attempt := 1; ; attempt++ {
		created, err := c.CreateToken(ctx, tokenRequest(plan))
		if err == nil {
			m := tokenFromAPI(&created.Token, plan)
			m.Token = types.StringValue(created.Secret)
			return m, diags
		}
		if errorCode(err) != quorumUnavailable {
			tokenError(&diags, summary, err)
			return plan, diags
		}
		if !revokeOrphan(ctx, c, plan, known, &diags) {
			return plan, diags
		}
		wait := retryDelay(attempt, err)
		if waited+wait > budget {
			apiError(&diags, fmt.Sprintf("%s: the cluster did not confirm the creation in %d attempts within retry_max_wait (the tokens it may have created were revoked); try again", summary, attempt), err)
			return plan, diags
		}
		waited += wait
		if err := tokenRetryWait(ctx, wait); err != nil {
			apiError(&diags, summary, err)
			return plan, diags
		}
	}
}

// retryDelay is the wait before the next attempt, as the connector does: the Retry-After of the answer, or 1, 2, 4…
// up to 64 seconds.
func retryDelay(attempt int, err error) time.Duration {
	if e, ok := err.(*taba.Error); ok && e.HasRetryAfter() {
		return time.Duration(e.RetryAfter) * time.Second
	}
	if attempt > 7 {
		attempt = 7
	}
	return time.Duration(1<<uint(attempt-1)) * time.Second
}

// tokenRetryWait waits d or until ctx ends (a variable, so the unit tests do not wait).
var tokenRetryWait = func(ctx context.Context, d time.Duration) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-time.After(d):
		return nil
	}
}

// sameName finds the token create_or_update takes over among the listed ones (the server filters by a part of the
// name): found when exactly one has the declared name and it has the declared permissions and prefixes. None with
// the name is not found, without a diagnostic (the token is created); several, or one with other permissions or
// prefixes, is an error, because taking over the wrong token would give its holder other rights.
func sameName(listed []taba.Token, name string, plan tokenModel, diags *diag.Diagnostics) (taba.Token, bool) {
	var named []taba.Token
	for _, t := range listed {
		if t.Name == name {
			named = append(named, t)
		}
	}
	switch {
	case len(named) == 0:
		return taba.Token{}, false
	case len(named) > 1:
		ids := make([]string, 0, len(named))
		for _, t := range named {
			ids = append(ids, t.ID)
		}
		sort.Strings(ids)
		diags.AddError(fmt.Sprintf("Several tokens are named %q", name),
			fmt.Sprintf("create_or_update takes over a token only when it is the only one with the name, and these have it: %s. Nothing was changed. Import the right one (terraform import <address> <id>), or revoke the extra ones.", strings.Join(ids, ", ")))
		return taba.Token{}, false
	case !tokenMatches(named[0], plan):
		t := named[0]
		diags.AddError(fmt.Sprintf("The token %q exists with other permissions or prefixes", name),
			fmt.Sprintf("The token %s has the permissions %v and the prefixes %v; the declared ones are %v and %v. Permissions and prefixes do not change in place, so it was not taken over and nothing was changed. "+
				"Declare what it has, import it and then change it (a replacement), or revoke it.",
				t.ID, t.Permissions, t.TopicPrefixes, stringSet(plan.Permissions), stringSet(plan.TopicPrefixes)))
		return taba.Token{}, false
	}
	return named[0], true
}

// adoptToken takes over an existing token for create_or_update: PATCH of expires_at when it differs, and the state
// without the secret, which the server only shows at the creation. The lookup and the PATCH are two calls, not one
// atomic decision as with the topics: two applies at the same time may each create a token with the name.
func adoptToken(ctx context.Context, c *taba.Client, plan tokenModel, existing taba.Token) (tokenModel, diag.Diagnostics) {
	var diags diag.Diagnostics
	t := &existing
	if want := serverDate(plan.ExpiresAt); !sameInstant(want, existing.ExpiresAt) {
		update := &taba.TokenUpdate{ExpiresAt: taba.SetString(want)}
		if want == "" {
			update.ExpiresAt = taba.ClearString()
		}
		updated, err := c.UpdateToken(ctx, existing.ID, update)
		if err != nil {
			if errorCode(err) == "token.not-found" {
				diags.AddError(fmt.Sprintf("The token %s no longer exists", existing.ID),
					"It was revoked or expired while it was being taken over. Run terraform apply again: the token is created.")
				return plan, diags
			}
			tokenError(&diags, fmt.Sprintf("Taking over the token %s", existing.ID), err)
			return plan, diags
		}
		t = updated
	}
	m := tokenFromAPI(t, plan)
	m.Token = types.StringNull()
	diags.AddWarning(fmt.Sprintf("Took over the existing token %q (%s)", plan.Name.ValueString(), existing.ID),
		"create_or_update is on and a token with this name, permissions and prefixes already existed: it belongs to this state now, and a destroy revokes it. "+
			"Its secret cannot be recovered, so the token attribute is null; whoever needs the secret gets a new token with terraform apply -replace=<address>.")
	return m, diags
}

// quorumUnavailable is the 503 of a definition whose owner did not answer (doc 19).
const quorumUnavailable = "cluster.quorum-unavailable"

// revokeOrphan revokes the token a lost creation answer left behind: the declared one that is not in known. false
// (with a diagnostic) when it cannot tell which one, or the revocation fails.
func revokeOrphan(ctx context.Context, c *taba.Client, plan tokenModel, known map[string]bool, diags *diag.Diagnostics) bool {
	name := plan.Name.ValueString()
	all, err := c.ListAllTokens(ctx, name)
	if err != nil {
		tokenError(diags, fmt.Sprintf("Creating the token %q: the cluster did not confirm it, and looking for the token it may have created failed (look for it by name and revoke it)", name), err)
		return false
	}
	var orphans []string
	for _, t := range all {
		if !known[t.ID] && tokenMatches(t, plan) {
			orphans = append(orphans, t.ID)
		}
	}
	switch len(orphans) {
	case 0:
		return true
	case 1:
		if err := c.RevokeToken(ctx, orphans[0]); err != nil && errorCode(err) != "token.not-found" {
			tokenError(diags, fmt.Sprintf("Creating the token %q: the cluster did not confirm it, and revoking the token %s it created without the secret failed (revoke it by hand)", name, orphans[0]), err)
			return false
		}
		known[orphans[0]] = true
		return true
	default:
		sort.Strings(orphans)
		diags.AddError(fmt.Sprintf("Creating the token %q: the cluster did not confirm it", name),
			fmt.Sprintf("Several tokens equal to the declared one appeared during the creation (%s): someone else is creating the same token. Nothing was revoked; check them and revoke the extra ones by hand.", strings.Join(orphans, ", ")))
		return false
	}
}

// readToken is GET /tokens/{id}; found is false when the token no longer exists: revoked, or expired (the server
// hides an expired token at once).
func readToken(ctx context.Context, c *taba.Client, state tokenModel) (tokenModel, bool, diag.Diagnostics) {
	var diags diag.Diagnostics
	id := state.ID.ValueString()
	t, err := c.GetToken(ctx, id)
	if err != nil {
		if errorCode(err) == "token.not-found" {
			return state, false, diags
		}
		tokenError(&diags, fmt.Sprintf("Reading the token %s", id), err)
		return state, false, diags
	}
	return tokenFromAPI(t, state), true, diags
}

// updateToken is PATCH /tokens/{id} with the name and the deadline (the rest replaces the token).
func updateToken(ctx context.Context, c *taba.Client, plan, state tokenModel) (tokenModel, diag.Diagnostics) {
	var diags diag.Diagnostics
	id := state.ID.ValueString()
	update := &taba.TokenUpdate{}
	if !plan.Name.Equal(state.Name) {
		name := plan.Name.ValueString()
		update.Name = &name
	}
	if !plan.ExpiresAt.Equal(state.ExpiresAt) {
		if plan.ExpiresAt.IsNull() {
			update.ExpiresAt = taba.ClearString()
		} else {
			update.ExpiresAt = taba.SetString(serverDate(plan.ExpiresAt))
		}
	}
	if update.Name == nil && update.ExpiresAt == nil {
		return plan, diags
	}
	t, err := c.UpdateToken(ctx, id, update)
	if err != nil {
		if errorCode(err) == "token.not-found" {
			diags.AddError(fmt.Sprintf("The token %s no longer exists", id),
				"It was revoked or expired after the last read. Run terraform apply again: the plan creates a new token.")
			return plan, diags
		}
		tokenError(&diags, fmt.Sprintf("Updating the token %s", id), err)
		return plan, diags
	}
	m := tokenFromAPI(t, plan)
	m.Token = state.Token
	return m, diags
}

// revokeToken is DELETE /tokens/{id}; a token already gone (revoked or expired) is success.
func revokeToken(ctx context.Context, c *taba.Client, id string) diag.Diagnostics {
	var diags diag.Diagnostics
	if err := c.RevokeToken(ctx, id); err != nil && errorCode(err) != "token.not-found" {
		tokenError(&diags, fmt.Sprintf("Revoking the token %s", id), err)
	}
	return diags
}

// -- dates ----------------------------------------------------------------------------------------------------------

// serverDate is a declared RFC 3339 date in the format the server accepts ("" when null).
func serverDate(v types.String) string {
	if v.IsNull() || v.IsUnknown() {
		return ""
	}
	at, err := time.Parse(time.RFC3339, v.ValueString())
	if err != nil {
		return v.ValueString()
	}
	return at.UTC().Format(serverTime)
}

// sameInstant says whether two RFC 3339 texts are the same instant, to the millisecond the server keeps.
func sameInstant(a, b string) bool {
	ta, errA := time.Parse(time.RFC3339, a)
	tb, errB := time.Parse(time.RFC3339, b)
	if errA != nil || errB != nil {
		return a == b
	}
	return ta.Truncate(time.Millisecond).Equal(tb.Truncate(time.Millisecond))
}

// sameInstantModifier plans expires_at as the declared value, except that a declared date that is the same instant
// as the one in the state keeps the state text: after an import, 2027-01-01T00:00:00Z and the server's
// 2027-01-01T00:00:00.000Z are not a change.
type sameInstantModifier struct{}

func (sameInstantModifier) Description(context.Context) string {
	return "The same instant written another way is not a change."
}

func (m sameInstantModifier) MarkdownDescription(ctx context.Context) string {
	return m.Description(ctx)
}

func (sameInstantModifier) PlanModifyString(_ context.Context, req planmodifier.StringRequest, resp *planmodifier.StringResponse) {
	switch {
	case req.ConfigValue.IsUnknown():
		resp.PlanValue = types.StringUnknown()
	case req.ConfigValue.IsNull():
		resp.PlanValue = types.StringNull()
	case !req.StateValue.IsNull() && !req.StateValue.IsUnknown() && sameInstant(req.ConfigValue.ValueString(), req.StateValue.ValueString()):
		resp.PlanValue = req.StateValue
	default:
		resp.PlanValue = req.ConfigValue
	}
}

// -- sets -----------------------------------------------------------------------------------------------------------

// stringSet is the sorted Go slice of a known set of strings (nil when null or unknown).
func stringSet(s types.Set) []string {
	if s.IsNull() || s.IsUnknown() {
		return nil
	}
	out := make([]string, 0, len(s.Elements()))
	for _, v := range s.Elements() {
		if str, ok := v.(types.String); ok {
			out = append(out, str.ValueString())
		}
	}
	sort.Strings(out)
	return out
}

func setValue(l []string) types.Set {
	elements := make([]attr.Value, 0, len(l))
	for _, v := range l {
		elements = append(elements, types.StringValue(v))
	}
	return types.SetValueMust(types.StringType, elements)
}

// sameStrings compares two lists as sets.
func sameStrings(a, b []string) bool {
	x := append([]string(nil), a...)
	y := append([]string(nil), b...)
	sort.Strings(x)
	sort.Strings(y)
	return strings.Join(x, "\x00") == strings.Join(y, "\x00")
}
