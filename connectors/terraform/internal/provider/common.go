package provider

import (
	"context"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
	"time"

	"github.com/hashicorp/terraform-plugin-framework/attr"
	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	rschema "github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/schema/validator"
	"github.com/hashicorp/terraform-plugin-framework/types"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

var (
	urlPattern = regexp.MustCompile(`^https?://`)
	// The rules of doc 16, "Rules for name, tags and ranges".
	namePattern     = regexp.MustCompile(`^[A-Za-z0-9_-]{1,100}$`)
	tagKeyPattern   = regexp.MustCompile(`^[A-Za-z0-9_.:-]{1,64}$`)
	tagValuePattern = regexp.MustCompile(`^[A-Za-z0-9_.:-]{0,128}$`)
	prefixPattern   = regexp.MustCompile(`^[A-Za-z0-9_-]{1,100}$`)
)

// deadLetterSuffix ends the name of the dead-letter queue that every queue gets; a topic cannot use it.
const deadLetterSuffix = "-dead-letter"

// errorCode is the code of a *taba.Error, or "" for any other error.
func errorCode(err error) string {
	if e, ok := err.(*taba.Error); ok {
		return e.Code
	}
	return ""
}

// apiError adds an error diagnostic for a failed call, with the server details when there are any.
func apiError(diags *diag.Diagnostics, summary string, err error) {
	detail := err.Error()
	if text := detailsText(err); text != "" {
		detail += "\n" + text
	}
	diags.AddError(summary, detail)
}

// detailsText is the details field of an ErrorResponse as lines "field: message" (the server sends a list of
// {field, code, message}); anything else comes as the server sent it. Empty when there are no details.
func detailsText(err error) string {
	e, ok := err.(*taba.Error)
	if !ok || e.Details == "" || e.Details == "null" {
		return ""
	}
	var items []struct {
		Field   string `json:"field"`
		Message string `json:"message"`
	}
	if json.Unmarshal([]byte(e.Details), &items) != nil || len(items) == 0 {
		return "Details: " + e.Details
	}
	lines := make([]string, 0, len(items))
	for _, item := range items {
		switch {
		case item.Field != "" && item.Message != "":
			lines = append(lines, "- "+item.Field+": "+item.Message)
		case item.Message != "":
			lines = append(lines, "- "+item.Message)
		}
	}
	if len(lines) == 0 {
		return "Details: " + e.Details
	}
	return strings.Join(lines, "\n")
}

// clientFrom takes the client out of the data Configure received. Nil data happens before the provider is
// configured (validation), when nothing calls the server yet.
func clientFrom(data any, diags *diag.Diagnostics) *taba.Client {
	if data == nil {
		return nil
	}
	d, ok := data.(*providerData)
	if !ok {
		diags.AddError("Unexpected provider data", fmt.Sprintf("expected *providerData, got %T", data))
		return nil
	}
	return d.client
}

// configureResource returns the client and the provider default of create_or_update.
func configureResource(req resource.ConfigureRequest, resp *resource.ConfigureResponse) (*taba.Client, bool) {
	client := clientFrom(req.ProviderData, &resp.Diagnostics)
	if d, ok := req.ProviderData.(*providerData); ok {
		return client, d.createOrUpdate
	}
	return client, false
}

func configureDataSource(req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) *taba.Client {
	return clientFrom(req.ProviderData, &resp.Diagnostics)
}

// -- conversions between the connector and the framework -----------------------------------------------------

// stringMap is the Go map of a known map of strings (nil when null or unknown).
func stringMap(m types.Map) map[string]string {
	if m.IsNull() || m.IsUnknown() {
		return nil
	}
	out := make(map[string]string, len(m.Elements()))
	for k, v := range m.Elements() {
		if s, ok := v.(types.String); ok {
			out[k] = s.ValueString()
		}
	}
	return out
}

func mapValue(m map[string]string) types.Map {
	elements := make(map[string]attr.Value, len(m))
	for k, v := range m {
		elements[k] = types.StringValue(v)
	}
	return types.MapValueMust(types.StringType, elements)
}

// stringList is the Go slice of a known list of strings (nil when null or unknown).
func stringList(l types.List) []string {
	if l.IsNull() || l.IsUnknown() {
		return nil
	}
	out := make([]string, 0, len(l.Elements()))
	for _, v := range l.Elements() {
		if s, ok := v.(types.String); ok {
			out = append(out, s.ValueString())
		}
	}
	return out
}

func listValue(l []string) types.List {
	elements := make([]attr.Value, 0, len(l))
	for _, v := range l {
		elements = append(elements, types.StringValue(v))
	}
	return types.ListValueMust(types.StringType, elements)
}

// intPointer is the connector form of an optional number: nil when null or unknown.
func intPointer(v types.Int64) *int {
	if v.IsNull() || v.IsUnknown() {
		return nil
	}
	n := int(v.ValueInt64())
	return &n
}

func int64Value(n *int) types.Int64 {
	if n == nil {
		return types.Int64Null()
	}
	return types.Int64Value(int64(*n))
}

func boolValue(b *bool) types.Bool {
	return types.BoolValue(b != nil && *b)
}

func stringOrNull(s string) types.String {
	if s == "" {
		return types.StringNull()
	}
	return types.StringValue(s)
}

// -- validators -------------------------------------------------------------------------------------------------

// topicNameValidator is the name of a topic: the pattern of doc 16 and not a dead-letter name.
type topicNameValidator struct{}

func (topicNameValidator) Description(context.Context) string {
	return "1 to 100 characters A-Za-z0-9_-, not ending in -dead-letter"
}

func (v topicNameValidator) MarkdownDescription(ctx context.Context) string {
	return v.Description(ctx)
}

func (topicNameValidator) ValidateString(_ context.Context, req validator.StringRequest, resp *validator.StringResponse) {
	if req.ConfigValue.IsNull() || req.ConfigValue.IsUnknown() {
		return
	}
	name := req.ConfigValue.ValueString()
	if !namePattern.MatchString(name) {
		resp.Diagnostics.AddAttributeError(req.Path, "Invalid topic name",
			fmt.Sprintf("%q must have 1 to 100 characters A-Za-z0-9_- (case-sensitive).", name))
		return
	}
	if strings.HasSuffix(name, deadLetterSuffix) {
		resp.Diagnostics.AddAttributeError(req.Path, "Reserved topic name",
			fmt.Sprintf("%q ends in %s: that name belongs to the dead-letter queue every queue gets, which is not a resource (read it with the taba_topic data source).", name, deadLetterSuffix))
	}
}

// -- create_or_update -----------------------------------------------------------------------------------------------

// createOrUpdateAttribute is the create_or_update attribute of a resource; takeOver says what it does on creation.
func createOrUpdateAttribute(takeOver string) rschema.BoolAttribute {
	return rschema.BoolAttribute{
		MarkdownDescription: takeOver + " Not set: the provider `create_or_update` (default `false`). " +
			"It is only used when the resource is created; afterwards the resource belongs to this state like any other, and a `destroy` removes it.",
		Optional: true,
	}
}

// wantsCreateOrUpdate is the create_or_update in force: the resource one, or the provider default.
func wantsCreateOrUpdate(v types.Bool, providerDefault bool) bool {
	if v.IsNull() || v.IsUnknown() {
		return providerDefault
	}
	return v.ValueBool()
}

// conflictAttempts bounds the PUT repeated after 409 cluster.conflict (the topic changed between the count and the
// recreation; the server asks to repeat after Retry-After: 1).
const conflictAttempts = 3

// adoptTopic is the create_or_update of a queue or distributor: PUT /topics/{name}?recreate=if-empty, decided by the
// definition owner at once. It creates the topic, or brings the existing one to the declared state (what is not
// declared goes back to the node default), or, when it has another type, fifo or volatile, recreates it only if it
// holds no message: the plan shows only a creation, so messages that are not of this state are never removed.
func adoptTopic(ctx context.Context, c *taba.Client, kind string, request *taba.TopicRequest, diags *diag.Diagnostics) (*taba.Topic, bool) {
	name := request.Name
	var put *taba.TopicPut
	var err error
	for attempt := 1; ; attempt++ {
		put, err = c.CreateOrUpdateTopic(ctx, request, taba.RecreateIfEmpty)
		if errorCode(err) != "cluster.conflict" || attempt == conflictAttempts {
			break
		}
		select {
		case <-ctx.Done():
			apiError(diags, fmt.Sprintf("Creating or taking over the %s %q", kind, name), ctx.Err())
			return nil, false
		case <-time.After(time.Second):
		}
	}
	if err != nil {
		switch errorCode(err) {
		case "topic.not-empty":
			detail := fmt.Sprintf("A topic named %q already exists with another type, fifo or volatile, and it (or its dead-letter queue) holds messages. create_or_update only recreates an empty topic, so nothing was changed. "+
				"To manage it here, import it (terraform import <address> %s): after the import, a change of type, fifo or volatile is a replacement that the plan shows, governed by force_destroy.", name, name)
			if text := detailsText(err); text != "" {
				detail += "\n" + text
			}
			diags.AddError(fmt.Sprintf("The topic %q exists with another configuration and is not empty", name), detail)
		case "cluster.quorum-unavailable":
			apiError(diags, fmt.Sprintf("Creating or taking over the %s %q: the topic exists with another type, fifo or volatile, and a node did not answer the message count, so nothing was changed; try again", kind, name), err)
		default:
			apiError(diags, fmt.Sprintf("Creating or taking over the %s %q", kind, name), err)
		}
		return nil, false
	}
	switch put.Outcome {
	case taba.TopicUpdated:
		diags.AddWarning(fmt.Sprintf("Took over the existing %s %q", kind, name),
			"create_or_update is on and the topic already existed: it now has the declared configuration (what is not declared went back to the node default) and belongs to this state. A destroy removes it.")
	case taba.TopicRecreated:
		diags.AddWarning(fmt.Sprintf("Recreated the topic %q as the declared %s", name, kind),
			"create_or_update is on and a topic with this name existed with another type, fifo or volatile, holding no message: it was removed and created again as declared, with a new id. It belongs to this state now.")
	}
	return &put.Topic, true
}

// alreadyExistsHint is the detail of a creation refused with 409 topic.already-exists.
func alreadyExistsHint(name string) string {
	return fmt.Sprintf("A topic named %q already exists in the cluster. To manage it here, import it (terraform import <address> %s, or an import block), or set create_or_update = true to take it over.", name, name)
}

// knownOrNull is v, or null when it is unknown (a state cannot keep an unknown value).
func knownOrNull(v types.Bool) types.Bool {
	if v.IsUnknown() {
		return types.BoolNull()
	}
	return v
}
