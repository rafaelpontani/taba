package provider

import (
	"context"
	"fmt"

	"github.com/hashicorp/terraform-plugin-framework-validators/listvalidator"
	"github.com/hashicorp/terraform-plugin-framework-validators/stringvalidator"
	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/schema/validator"
	"github.com/hashicorp/terraform-plugin-framework/types"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

var (
	_ resource.Resource                = (*distributorResource)(nil)
	_ resource.ResourceWithConfigure   = (*distributorResource)(nil)
	_ resource.ResourceWithImportState = (*distributorResource)(nil)
)

func newDistributorResource() resource.Resource { return &distributorResource{} }

type distributorResource struct {
	client         *taba.Client
	createOrUpdate bool
}

type distributorModel struct {
	ID             types.String `tfsdk:"id"`
	Name           types.String `tfsdk:"name"`
	Prefixes       types.List   `tfsdk:"prefixes"`
	Tags           types.Map    `tfsdk:"tags"`
	CreateOrUpdate types.Bool   `tfsdk:"create_or_update"`
	CreatedAt      types.String `tfsdk:"created_at"`
}

func (r *distributorResource) Metadata(_ context.Context, req resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_distributor"
}

func (r *distributorResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "A distributor: each publication to it is copied to every queue whose name starts with one of the `prefixes`. It holds no messages.",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				MarkdownDescription: "Identifier the server gave the distributor.",
				Computed:            true,
				PlanModifiers:       []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
			"name": schema.StringAttribute{
				MarkdownDescription: "Name: 1 to 100 characters `A-Za-z0-9_-`, case-sensitive, not ending in `-dead-letter`. Changing it replaces the distributor.",
				Required:            true,
				Validators:          []validator.String{topicNameValidator{}},
				PlanModifiers:       []planmodifier.String{stringplanmodifier.RequiresReplace()},
			},
			"prefixes": schema.ListAttribute{
				MarkdownDescription: "Name prefixes of the queues each publication is copied to (1 to 20, of 1 to 100 characters `A-Za-z0-9_-`). A change applies to the publications after it.",
				ElementType:         types.StringType,
				Required:            true,
				Validators: []validator.List{
					listvalidator.SizeBetween(1, 20),
					listvalidator.ValueStringsAre(stringvalidator.RegexMatches(prefixPattern, "must have 1 to 100 characters A-Za-z0-9_-")),
				},
			},
			"tags":             tagsAttribute(),
			"create_or_update": createOrUpdateAttribute("When creating, take over a topic with this name that already exists instead of failing: it gets the declared `prefixes` and `tags`. A queue with this name is recreated as a distributor only if it holds no message; otherwise nothing changes and the creation fails."),
			"created_at": schema.StringAttribute{
				MarkdownDescription: "Creation date (RFC 3339).",
				Computed:            true,
				PlanModifiers:       []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
		},
	}
}

func (r *distributorResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	r.client, r.createOrUpdate = configureResource(req, resp)
}

func (r *distributorResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan distributorModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	created, diags := createDistributor(ctx, r.client, plan, wantsCreateOrUpdate(plan.CreateOrUpdate, r.createOrUpdate))
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, created)...)
}

func (r *distributorResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state distributorModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	var diags diag.Diagnostics
	t, found := readTopic(ctx, r.client, state.Name.ValueString(), "distributor", "taba_queue", &diags)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	if !found {
		resp.State.RemoveResource(ctx)
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, distributorFromTopic(t, state))...)
}

func (r *distributorResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan distributorModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	updated, diags := updateDistributor(ctx, r.client, plan)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, updated)...)
}

// Delete is a plain DELETE: a distributor holds no messages.
func (r *distributorResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state distributorModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(deleteTopic(ctx, r.client, "distributor", state.Name.ValueString(), true)...)
}

// ImportState takes the distributor name: terraform import taba_distributor.fanout fanout.
func (r *distributorResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("name"), req, resp)
}

// -- the calls ------------------------------------------------------------------------------------------------------

func distributorRequest(m distributorModel) *taba.TopicRequest {
	return &taba.TopicRequest{
		Name:     m.Name.ValueString(),
		Type:     "distributor",
		Tags:     stringMap(m.Tags),
		Prefixes: stringList(m.Prefixes),
	}
}

// distributorFromTopic is the state of the distributor as the server returned it; create_or_update comes from prior.
func distributorFromTopic(t *taba.Topic, prior distributorModel) distributorModel {
	return distributorModel{
		CreateOrUpdate: knownOrNull(prior.CreateOrUpdate),
		ID:             types.StringValue(t.ID),
		Name:           types.StringValue(t.Name),
		Prefixes:       listValue(t.Prefixes),
		Tags:           mapValue(t.Tags),
		CreatedAt:      stringOrNull(t.CreatedAt),
	}
}

func distributorDifferences(m distributorModel, t *taba.Topic) []string {
	if t.Type != "distributor" {
		return []string{"type (it is a " + t.Type + ")"}
	}
	got := distributorFromTopic(t, m)
	var diffs []string
	if !m.Prefixes.IsUnknown() && !m.Prefixes.Equal(got.Prefixes) {
		diffs = append(diffs, "prefixes")
	}
	if !m.Tags.IsUnknown() && !m.Tags.Equal(got.Tags) {
		diffs = append(diffs, "tags")
	}
	return diffs
}

// createDistributor is POST /topics (or the PUT of adoptTopic with create_or_update), with the same recovery after a 503 as createQueue.
func createDistributor(ctx context.Context, c *taba.Client, plan distributorModel, createOrUpdate bool) (distributorModel, diag.Diagnostics) {
	var diags diag.Diagnostics
	name := plan.Name.ValueString()
	if createOrUpdate {
		t, ok := adoptTopic(ctx, c, "distributor", distributorRequest(plan), &diags)
		if !ok {
			return plan, diags
		}
		return distributorFromTopic(t, plan), diags
	}
	t, err := c.CreateTopic(ctx, distributorRequest(plan))
	if err != nil {
		if errorCode(err) == "topic.already-exists" {
			diags.AddError("The distributor already exists", alreadyExistsHint(name))
			return plan, diags
		}
		apiError(&diags, fmt.Sprintf("Creating the distributor %q", name), err)
		return plan, diags
	}
	if diffs := distributorDifferences(plan, t); len(diffs) > 0 {
		diags.AddError("The distributor already exists with another configuration",
			fmt.Sprintf("The creation of %q was interrupted (503) and, on the retry, the topic already existed with other values for %v: someone else created it at the same time. Nothing was changed. Import it to manage it here: terraform import <address> %s.", name, diffs, name))
		return plan, diags
	}
	return distributorFromTopic(t, plan), diags
}

// updateDistributor is PUT /topics/{name} with the declared prefixes and tags (see updateQueue).
func updateDistributor(ctx context.Context, c *taba.Client, plan distributorModel) (distributorModel, diag.Diagnostics) {
	var diags diag.Diagnostics
	name := plan.Name.ValueString()
	put, err := c.CreateOrUpdateTopic(ctx, distributorRequest(plan), taba.RecreateNever)
	if err != nil {
		apiError(&diags, fmt.Sprintf("Updating the distributor %q", name), err)
		return plan, diags
	}
	if put.Outcome != taba.TopicUpdated {
		diags.AddWarning("The distributor was created again",
			fmt.Sprintf("The distributor %q no longer existed when it was updated (someone removed it after the last read), so it was created again with a new id.", name))
	}
	return distributorFromTopic(&put.Topic, plan), diags
}
