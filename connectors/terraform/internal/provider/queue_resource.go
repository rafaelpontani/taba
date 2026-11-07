package provider

import (
	"context"
	"encoding/json"
	"fmt"
	"sort"

	"github.com/hashicorp/terraform-plugin-framework-validators/int64validator"
	"github.com/hashicorp/terraform-plugin-framework-validators/mapvalidator"
	"github.com/hashicorp/terraform-plugin-framework-validators/stringvalidator"
	"github.com/hashicorp/terraform-plugin-framework/attr"
	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/booldefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/boolplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/mapdefault"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/schema/validator"
	"github.com/hashicorp/terraform-plugin-framework/types"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

// defaultIdleTimeout is the idleTimeout the server gives a volatile queue that does not declare one (doc 16).
const defaultIdleTimeout = 600

var (
	_ resource.Resource                   = (*queueResource)(nil)
	_ resource.ResourceWithConfigure      = (*queueResource)(nil)
	_ resource.ResourceWithImportState    = (*queueResource)(nil)
	_ resource.ResourceWithValidateConfig = (*queueResource)(nil)
)

func newQueueResource() resource.Resource { return &queueResource{} }

type queueResource struct {
	client         *taba.Client
	createOrUpdate bool
}

type queueModel struct {
	ID                       types.String `tfsdk:"id"`
	Name                     types.String `tfsdk:"name"`
	Fifo                     types.Bool   `tfsdk:"fifo"`
	Volatile                 types.Bool   `tfsdk:"volatile"`
	Tags                     types.Map    `tfsdk:"tags"`
	TTL                      types.Int64  `tfsdk:"ttl"`
	MaxDeliveries            types.Int64  `tfsdk:"max_deliveries"`
	MaxMessageBytes          types.Int64  `tfsdk:"max_message_bytes"`
	IdleTimeout              types.Int64  `tfsdk:"idle_timeout"`
	ForceDestroy             types.Bool   `tfsdk:"force_destroy"`
	CreateOrUpdate           types.Bool   `tfsdk:"create_or_update"`
	CreatedAt                types.String `tfsdk:"created_at"`
	DeadLetterName           types.String `tfsdk:"dead_letter_name"`
	EffectiveTTL             types.Int64  `tfsdk:"effective_ttl"`
	EffectiveMaxDeliveries   types.Int64  `tfsdk:"effective_max_deliveries"`
	EffectiveMaxMessageBytes types.Int64  `tfsdk:"effective_max_message_bytes"`
}

func (r *queueResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_queue"
}

func tagsAttribute() schema.MapAttribute {
	return schema.MapAttribute{
		MarkdownDescription: "Telemetry tags (up to 20; key of 1 to 64 and value of 0 to 128 characters `A-Za-z0-9_-.:`). They do not affect behaviour. Default: none.",
		ElementType:         types.StringType,
		Optional:            true,
		Computed:            true,
		Default:             mapdefault.StaticValue(types.MapValueMust(types.StringType, map[string]attr.Value{})),
		Validators: []validator.Map{
			mapvalidator.SizeAtMost(20),
			mapvalidator.KeysAre(stringvalidator.RegexMatches(tagKeyPattern, "must have 1 to 64 characters A-Za-z0-9_-.:")),
			mapvalidator.ValueStringsAre(stringvalidator.RegexMatches(tagValuePattern, "must have up to 128 characters A-Za-z0-9_-.:")),
		},
	}
}

func (r *queueResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "A queue. Every queue gets a dead-letter queue (`dead_letter_name`), created and removed with it.\n\n" +
			"Changing `name`, `fifo` or `volatile` replaces the queue, and removing a queue removes its messages: " +
			"unless `force_destroy` is `true`, the removal (`destroy`, or the replacement) is refused while the queue or its dead-letter holds a message.",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				MarkdownDescription: "Identifier the server gave the queue (a new one when the queue is replaced).",
				Computed:            true,
				PlanModifiers:       []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
			"name": schema.StringAttribute{
				MarkdownDescription: "Name: 1 to 100 characters `A-Za-z0-9_-`, case-sensitive, not ending in `-dead-letter`. Changing it replaces the queue.",
				Required:            true,
				Validators:          []validator.String{topicNameValidator{}},
				PlanModifiers:       []planmodifier.String{stringplanmodifier.RequiresReplace()},
			},
			"fifo": schema.BoolAttribute{
				MarkdownDescription: "FIFO queue (order per group and deduplication). Needs `replication.factor` of 2 or more on the cluster. Changing it replaces the queue. Default: `false`.",
				Optional:            true,
				Computed:            true,
				Default:             booldefault.StaticBool(false),
				PlanModifiers:       []planmodifier.Bool{boolplanmodifier.RequiresReplace()},
			},
			"volatile": schema.BoolAttribute{
				MarkdownDescription: "Volatile queue: messages only in memory, and the queue is removed after `idle_timeout` without a consumer. Changing it replaces the queue. Default: `false`.",
				Optional:            true,
				Computed:            true,
				Default:             booldefault.StaticBool(false),
				PlanModifiers:       []planmodifier.Bool{boolplanmodifier.RequiresReplace()},
			},
			"tags": tagsAttribute(),
			"ttl": schema.Int64Attribute{
				MarkdownDescription: "Message lifetime, in seconds (60 to 31536000). Not set: the node default (`effective_ttl`).",
				Optional:            true,
				Validators:          []validator.Int64{int64validator.Between(60, 31536000)},
			},
			"max_deliveries": schema.Int64Attribute{
				MarkdownDescription: "Deliveries without an ack before the message goes to the dead-letter (1 to 1000). Not set: the node default (`effective_max_deliveries`).",
				Optional:            true,
				Validators:          []validator.Int64{int64validator.Between(1, 1000)},
			},
			"max_message_bytes": schema.Int64Attribute{
				MarkdownDescription: "Maximum size of a message, in bytes (1 up to the node `message.hard-limit`). Not set: the node default (`effective_max_message_bytes`).",
				Optional:            true,
				Validators:          []validator.Int64{int64validator.AtLeast(1)},
			},
			"idle_timeout": schema.Int64Attribute{
				MarkdownDescription: fmt.Sprintf("Volatile queue only: seconds without a consumer until the queue is removed (60 to 604800). Default on a volatile queue: `%d`.", defaultIdleTimeout),
				Optional:            true,
				Computed:            true,
				Validators:          []validator.Int64{int64validator.Between(60, 604800)},
				PlanModifiers:       []planmodifier.Int64{idleTimeoutModifier{}},
			},
			"force_destroy": schema.BoolAttribute{
				MarkdownDescription: "Remove the queue even when it holds messages (they are lost). With `false`, the removal is refused while the queue or its dead-letter holds a message. It is read from the state, so set it to `true` and apply before a `destroy` or a replacement. Default: `false`.",
				Optional:            true,
				Computed:            true,
				Default:             booldefault.StaticBool(false),
			},
			"create_or_update": createOrUpdateAttribute("When creating, take over a topic with this name that already exists instead of failing: it gets the declared configuration (what is not declared goes back to the node default). A topic with another type, `fifo` or `volatile` is recreated only if it holds no message; otherwise nothing changes and the creation fails."),
			"created_at": schema.StringAttribute{
				MarkdownDescription: "Creation date (RFC 3339).",
				Computed:            true,
				PlanModifiers:       []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
			"dead_letter_name": schema.StringAttribute{
				MarkdownDescription: "Name of the dead-letter queue of this queue (`<name>-dead-letter`).",
				Computed:            true,
				PlanModifiers:       []planmodifier.String{deadLetterNameModifier{}},
			},
			"effective_ttl": schema.Int64Attribute{
				MarkdownDescription: "The `ttl` in force: the declared one or the node default.",
				Computed:            true,
				PlanModifiers:       []planmodifier.Int64{effectiveModifier{field: "ttl"}},
			},
			"effective_max_deliveries": schema.Int64Attribute{
				MarkdownDescription: "The `max_deliveries` in force: the declared one or the node default.",
				Computed:            true,
				PlanModifiers:       []planmodifier.Int64{effectiveModifier{field: "max_deliveries"}},
			},
			"effective_max_message_bytes": schema.Int64Attribute{
				MarkdownDescription: "The `max_message_bytes` in force: the declared one or the node default.",
				Computed:            true,
				PlanModifiers:       []planmodifier.Int64{effectiveModifier{field: "max_message_bytes"}},
			},
		},
	}
}

func (r *queueResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	r.client, r.createOrUpdate = configureResource(req, resp)
}

func (r *queueResource) ValidateConfig(ctx context.Context, req resource.ValidateConfigRequest, resp *resource.ValidateConfigResponse) {
	var m queueModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(validateQueue(m)...)
}

// validateQueue is what the schema cannot say alone: idle_timeout only on a volatile queue.
func validateQueue(m queueModel) diag.Diagnostics {
	var diags diag.Diagnostics
	if !m.IdleTimeout.IsNull() && !m.IdleTimeout.IsUnknown() && !m.Volatile.IsUnknown() && !m.Volatile.ValueBool() {
		diags.AddAttributeError(path.Root("idle_timeout"), "idle_timeout needs a volatile queue",
			"idle_timeout only applies to a volatile queue: set volatile = true or remove idle_timeout.")
	}
	return diags
}

func (r *queueResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan queueModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	created, diags := createQueue(ctx, r.client, plan, wantsCreateOrUpdate(plan.CreateOrUpdate, r.createOrUpdate))
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, created)...)
}

func (r *queueResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state queueModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	read, found, diags := readQueue(ctx, r.client, state)
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

func (r *queueResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan queueModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	updated, diags := updateQueue(ctx, r.client, plan)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, updated)...)
}

func (r *queueResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state queueModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(deleteTopic(ctx, r.client, "queue", state.Name.ValueString(), state.ForceDestroy.ValueBool())...)
}

// ImportState takes the queue name: terraform import taba_queue.orders orders.
func (r *queueResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("name"), req, resp)
}

// -- the calls ------------------------------------------------------------------------------------------------------

// queueRequest is the body of POST and PUT /topics for the declared queue. What is not declared stays out, so the
// server applies its default (and the PUT, a full replacement, takes the field back to it).
func queueRequest(m queueModel) *taba.TopicRequest {
	r := &taba.TopicRequest{
		Name:            m.Name.ValueString(),
		Type:            "queue",
		Tags:            stringMap(m.Tags),
		TTL:             intPointer(m.TTL),
		MaxDeliveries:   intPointer(m.MaxDeliveries),
		MaxMessageBytes: intPointer(m.MaxMessageBytes),
	}
	if m.Fifo.ValueBool() {
		r.Fifo = taba.Bool(true)
	}
	if m.Volatile.ValueBool() {
		r.Volatile = taba.Bool(true)
		r.IdleTimeout = intPointer(m.IdleTimeout)
	}
	return r
}

// queueFromTopic is the state of the queue as the server returned it; force_destroy, which only the provider has,
// comes from prior (false when it has none, as after an import).
func queueFromTopic(t *taba.Topic, prior queueModel) queueModel {
	ttl, deliveries, bytes := effectiveOf(t.Raw)
	m := queueModel{
		ID:                       types.StringValue(t.ID),
		Name:                     types.StringValue(t.Name),
		Fifo:                     boolValue(t.Fifo),
		Volatile:                 boolValue(t.Volatile),
		Tags:                     mapValue(t.Tags),
		TTL:                      int64Value(t.TTL),
		MaxDeliveries:            int64Value(t.MaxDeliveries),
		MaxMessageBytes:          int64Value(t.MaxMessageBytes),
		IdleTimeout:              int64Value(t.IdleTimeout),
		ForceDestroy:             prior.ForceDestroy,
		CreateOrUpdate:           knownOrNull(prior.CreateOrUpdate),
		CreatedAt:                stringOrNull(t.CreatedAt),
		DeadLetterName:           types.StringValue(t.Name + deadLetterSuffix),
		EffectiveTTL:             int64Value(ttl),
		EffectiveMaxDeliveries:   int64Value(deliveries),
		EffectiveMaxMessageBytes: int64Value(bytes),
	}
	if m.ForceDestroy.IsNull() || m.ForceDestroy.IsUnknown() {
		m.ForceDestroy = types.BoolValue(false)
	}
	return m
}

// effectiveOf reads the values in force ("effective") of the topic JSON; nil where they are missing.
func effectiveOf(raw string) (ttl, maxDeliveries, maxMessageBytes *int) {
	var t struct {
		Effective struct {
			TTL             *int `json:"ttl"`
			MaxDeliveries   *int `json:"maxDeliveries"`
			MaxMessageBytes *int `json:"maxMessageBytes"`
		} `json:"effective"`
	}
	if json.Unmarshal([]byte(raw), &t) != nil {
		return nil, nil, nil
	}
	return t.Effective.TTL, t.Effective.MaxDeliveries, t.Effective.MaxMessageBytes
}

// queueDifferences lists the declared fields the topic does not have, sorted.
func queueDifferences(m queueModel, t *taba.Topic) []string {
	var diffs []string
	if t.Type != "queue" {
		return []string{"type (it is a " + t.Type + ")"}
	}
	got := queueFromTopic(t, m)
	check := func(name string, want, have attr.Value) {
		if !want.IsUnknown() && !want.Equal(have) {
			diffs = append(diffs, name)
		}
	}
	check("fifo", m.Fifo, got.Fifo)
	check("volatile", m.Volatile, got.Volatile)
	check("tags", m.Tags, got.Tags)
	check("ttl", m.TTL, got.TTL)
	check("max_deliveries", m.MaxDeliveries, got.MaxDeliveries)
	check("max_message_bytes", m.MaxMessageBytes, got.MaxMessageBytes)
	if m.Volatile.ValueBool() {
		check("idle_timeout", m.IdleTimeout, got.IdleTimeout)
	}
	sort.Strings(diffs)
	return diffs
}

// createQueue is POST /topics, or the PUT of adoptTopic with create_or_update. After a 503 the connector repeats the call and, on 409 topic.already-exists, reads the
// topic (the creation completed in the background): it is ours only if it is what was declared.
func createQueue(ctx context.Context, c *taba.Client, plan queueModel, createOrUpdate bool) (queueModel, diag.Diagnostics) {
	var diags diag.Diagnostics
	name := plan.Name.ValueString()
	if createOrUpdate {
		t, ok := adoptTopic(ctx, c, "queue", queueRequest(plan), &diags)
		if !ok {
			return plan, diags
		}
		return queueFromTopic(t, plan), diags
	}
	t, err := c.CreateTopic(ctx, queueRequest(plan))
	if err != nil {
		if errorCode(err) == "topic.already-exists" {
			diags.AddError("The queue already exists", alreadyExistsHint(name))
			return plan, diags
		}
		apiError(&diags, fmt.Sprintf("Creating the queue %q", name), err)
		return plan, diags
	}
	if diffs := queueDifferences(plan, t); len(diffs) > 0 {
		diags.AddError("The queue already exists with another configuration",
			fmt.Sprintf("The creation of %q was interrupted (503) and, on the retry, the topic already existed with other values for %v: someone else created it at the same time. Nothing was changed. Import it to manage it here: terraform import <address> %s.", name, diffs, name))
		return plan, diags
	}
	return queueFromTopic(t, plan), diags
}

// readQueue is GET /topics/{name}; found is false when the topic no longer exists (Terraform then plans to create it).
func readQueue(ctx context.Context, c *taba.Client, state queueModel) (queueModel, bool, diag.Diagnostics) {
	var diags diag.Diagnostics
	t, ok := readTopic(ctx, c, state.Name.ValueString(), "queue", "taba_distributor", &diags)
	if !ok {
		return state, false, diags
	}
	return queueFromTopic(t, state), true, diags
}

// updateQueue is PUT /topics/{name} with the whole declared state: what is no longer declared goes back to the node
// default (PATCH cannot do it for idleTimeout). The immutable fields do not change here (RequiresReplace), so the
// answer is a 200; a 201 means someone removed the queue after the last read, and it was created again.
func updateQueue(ctx context.Context, c *taba.Client, plan queueModel) (queueModel, diag.Diagnostics) {
	var diags diag.Diagnostics
	name := plan.Name.ValueString()
	put, err := c.CreateOrUpdateTopic(ctx, queueRequest(plan), taba.RecreateNever)
	if err != nil {
		apiError(&diags, fmt.Sprintf("Updating the queue %q", name), err)
		return plan, diags
	}
	if put.Outcome != taba.TopicUpdated {
		diags.AddWarning("The queue was created again",
			fmt.Sprintf("The queue %q no longer existed when it was updated (someone removed it after the last read), so it was created again, empty, with a new id.", name))
	}
	return queueFromTopic(&put.Topic, plan), diags
}

// readTopic reads a topic that must be of the given type. ok is false when it does not exist (no diagnostic) or on
// an error (with a diagnostic).
func readTopic(ctx context.Context, c *taba.Client, name, kind, otherResource string, diags *diag.Diagnostics) (*taba.Topic, bool) {
	t, err := c.GetTopic(ctx, name)
	if err != nil {
		if errorCode(err) == "topic.not-found" {
			return nil, false
		}
		apiError(diags, fmt.Sprintf("Reading the %s %q", kind, name), err)
		return nil, false
	}
	if t.DeadLetter != nil && *t.DeadLetter {
		diags.AddError("A dead-letter queue is not a resource",
			fmt.Sprintf("%q is the dead-letter queue of %q: it is created and removed with it. Read it with the taba_topic data source.", name, t.Source))
		return nil, false
	}
	if t.Type != kind {
		diags.AddError(fmt.Sprintf("%q is not a %s", name, kind),
			fmt.Sprintf("The topic %q is a %s: manage it with %s.", name, t.Type, otherResource))
		return nil, false
	}
	return t, true
}

// deleteTopic is DELETE /topics/{name}: with ?ifEmpty=true unless force, so a queue with messages is not lost by a
// destroy or a replacement. A topic already gone is success.
func deleteTopic(ctx context.Context, c *taba.Client, kind, name string, force bool) diag.Diagnostics {
	var diags diag.Diagnostics
	var err error
	if force {
		err = c.DeleteTopic(ctx, name)
	} else {
		err = c.DeleteTopicIfEmpty(ctx, name)
	}
	switch errorCode(err) {
	case "":
		if err != nil {
			apiError(&diags, fmt.Sprintf("Removing the %s %q", kind, name), err)
		}
	case "topic.not-found":
	case "topic.not-empty":
		detail := fmt.Sprintf("The %s %q (or its dead-letter queue) still holds messages, and removing it would lose them. Consume or prune them, or set force_destroy = true and apply before removing it.", kind, name)
		if text := detailsText(err); text != "" {
			detail += "\n" + text
		}
		diags.AddError(fmt.Sprintf("The %s is not empty", kind), detail)
	case "cluster.quorum-unavailable":
		apiError(&diags, fmt.Sprintf("Removing the %s %q: a node did not answer the message count, so nothing was removed; try again", kind, name), err)
	default:
		apiError(&diags, fmt.Sprintf("Removing the %s %q", kind, name), err)
	}
	return diags
}
