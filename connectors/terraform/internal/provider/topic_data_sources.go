package provider

import (
	"context"
	"fmt"
	"sort"

	"github.com/hashicorp/terraform-plugin-framework-validators/stringvalidator"
	"github.com/hashicorp/terraform-plugin-framework/attr"
	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/datasource/schema"
	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/schema/validator"
	"github.com/hashicorp/terraform-plugin-framework/types"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

var (
	_ datasource.DataSourceWithConfigure = (*topicDataSource)(nil)
	_ datasource.DataSourceWithConfigure = (*topicsDataSource)(nil)
)

// topicModel is a topic as the data sources show it: a queue, a distributor or a dead-letter queue.
type topicModel struct {
	ID                       types.String `tfsdk:"id"`
	Name                     types.String `tfsdk:"name"`
	Type                     types.String `tfsdk:"type"`
	Fifo                     types.Bool   `tfsdk:"fifo"`
	Volatile                 types.Bool   `tfsdk:"volatile"`
	Tags                     types.Map    `tfsdk:"tags"`
	Prefixes                 types.List   `tfsdk:"prefixes"`
	TTL                      types.Int64  `tfsdk:"ttl"`
	MaxDeliveries            types.Int64  `tfsdk:"max_deliveries"`
	MaxMessageBytes          types.Int64  `tfsdk:"max_message_bytes"`
	IdleTimeout              types.Int64  `tfsdk:"idle_timeout"`
	EffectiveTTL             types.Int64  `tfsdk:"effective_ttl"`
	EffectiveMaxDeliveries   types.Int64  `tfsdk:"effective_max_deliveries"`
	EffectiveMaxMessageBytes types.Int64  `tfsdk:"effective_max_message_bytes"`
	DeadLetter               types.Bool   `tfsdk:"dead_letter"`
	Source                   types.String `tfsdk:"source"`
	CreatedAt                types.String `tfsdk:"created_at"`
}

func topicFromAPI(t *taba.Topic) topicModel {
	ttl, deliveries, bytes := effectiveOf(t.Raw)
	m := topicModel{
		ID:                       types.StringValue(t.ID),
		Name:                     types.StringValue(t.Name),
		Type:                     types.StringValue(t.Type),
		Fifo:                     boolValue(t.Fifo),
		Volatile:                 boolValue(t.Volatile),
		Tags:                     mapValue(t.Tags),
		Prefixes:                 types.ListNull(types.StringType),
		TTL:                      int64Value(t.TTL),
		MaxDeliveries:            int64Value(t.MaxDeliveries),
		MaxMessageBytes:          int64Value(t.MaxMessageBytes),
		IdleTimeout:              int64Value(t.IdleTimeout),
		EffectiveTTL:             int64Value(ttl),
		EffectiveMaxDeliveries:   int64Value(deliveries),
		EffectiveMaxMessageBytes: int64Value(bytes),
		DeadLetter:               boolValue(t.DeadLetter),
		Source:                   stringOrNull(t.Source),
		CreatedAt:                stringOrNull(t.CreatedAt),
	}
	if t.Type == "distributor" {
		m.Prefixes = listValue(t.Prefixes)
	}
	return m
}

// topicAttributes are the computed attributes of a topic; name is set apart because taba_topic takes it as input.
func topicAttributes(nameRequired bool) map[string]schema.Attribute {
	computedInt := func(description string) schema.Int64Attribute {
		return schema.Int64Attribute{MarkdownDescription: description, Computed: true}
	}
	name := schema.StringAttribute{MarkdownDescription: "Name of the topic.", Computed: true}
	if nameRequired {
		name = schema.StringAttribute{MarkdownDescription: "Name of the topic (a dead-letter queue too).", Required: true}
	}
	return map[string]schema.Attribute{
		"id":                          schema.StringAttribute{MarkdownDescription: "Identifier of the topic.", Computed: true},
		"name":                        name,
		"type":                        schema.StringAttribute{MarkdownDescription: "`queue` or `distributor`.", Computed: true},
		"fifo":                        schema.BoolAttribute{MarkdownDescription: "FIFO queue.", Computed: true},
		"volatile":                    schema.BoolAttribute{MarkdownDescription: "Volatile queue.", Computed: true},
		"tags":                        schema.MapAttribute{MarkdownDescription: "Telemetry tags.", ElementType: types.StringType, Computed: true},
		"prefixes":                    schema.ListAttribute{MarkdownDescription: "Distributor only: the destination prefixes (null on a queue).", ElementType: types.StringType, Computed: true},
		"ttl":                         computedInt("Declared message lifetime, in seconds (null: the node default)."),
		"max_deliveries":              computedInt("Declared deliveries before the dead-letter (null: the node default)."),
		"max_message_bytes":           computedInt("Declared maximum message size (null: the node default)."),
		"idle_timeout":                computedInt("Volatile queue only: seconds without a consumer until removal."),
		"effective_ttl":               computedInt("The ttl in force."),
		"effective_max_deliveries":    computedInt("The max_deliveries in force."),
		"effective_max_message_bytes": computedInt("The max_message_bytes in force."),
		"dead_letter":                 schema.BoolAttribute{MarkdownDescription: "True on a dead-letter queue.", Computed: true},
		"source":                      schema.StringAttribute{MarkdownDescription: "Dead-letter queue only: the queue it belongs to.", Computed: true},
		"created_at":                  schema.StringAttribute{MarkdownDescription: "Creation date (RFC 3339).", Computed: true},
	}
}

// -- taba_topic -------------------------------------------------------------------------------------------------

func newTopicDataSource() datasource.DataSource { return &topicDataSource{} }

type topicDataSource struct {
	client *taba.Client
}

func (d *topicDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_topic"
}

func (d *topicDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Reads one topic by name: a queue, a distributor or a dead-letter queue, managed here or not. The token needs to reach the name (its prefixes).",
		Attributes:          topicAttributes(true),
	}
}

func (d *topicDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	d.client = configureDataSource(req, resp)
}

func (d *topicDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	var m topicModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	t, err := d.client.GetTopic(ctx, m.Name.ValueString())
	if err != nil {
		apiError(&resp.Diagnostics, fmt.Sprintf("Reading the topic %q", m.Name.ValueString()), err)
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, topicFromAPI(t))...)
}

// -- taba_topics ------------------------------------------------------------------------------------------------

func newTopicsDataSource() datasource.DataSource { return &topicsDataSource{} }

type topicsDataSource struct {
	client *taba.Client
}

type topicsModel struct {
	NameContains       types.String `tfsdk:"name_contains"`
	Type               types.String `tfsdk:"type"`
	Tags               types.Map    `tfsdk:"tags"`
	IncludeDeadLetters types.Bool   `tfsdk:"include_dead_letters"`
	Names              types.List   `tfsdk:"names"`
	Topics             []topicModel `tfsdk:"topics"`
}

func (d *topicsDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_topics"
}

func (d *topicsDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Lists the topics the token reaches, sorted by name, with optional filters.",
		Attributes: map[string]schema.Attribute{
			"name_contains": schema.StringAttribute{
				MarkdownDescription: "Only the topics whose name contains this text (case-sensitive).",
				Optional:            true,
			},
			"type": schema.StringAttribute{
				MarkdownDescription: "Only `queue` or only `distributor`.",
				Optional:            true,
				Validators:          []validator.String{stringvalidator.OneOf("queue", "distributor")},
			},
			"tags": schema.MapAttribute{
				MarkdownDescription: "Only the topics that have all these tags, with these values.",
				ElementType:         types.StringType,
				Optional:            true,
			},
			"include_dead_letters": schema.BoolAttribute{
				MarkdownDescription: "Also list the dead-letter queues. Default: `false`.",
				Optional:            true,
			},
			"names": schema.ListAttribute{
				MarkdownDescription: "The names of the topics found.",
				ElementType:         types.StringType,
				Computed:            true,
			},
			"topics": schema.ListNestedAttribute{
				MarkdownDescription: "The topics found.",
				Computed:            true,
				NestedObject:        schema.NestedAttributeObject{Attributes: topicAttributes(false)},
			},
		},
	}
}

func (d *topicsDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	d.client = configureDataSource(req, resp)
}

func (d *topicsDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	var m topicsModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(listTopics(ctx, d.client, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, m)...)
}

// listTopics fills names and topics of m with the topics that pass its filters. The name filter goes to the
// server (GET /topics?name=, "contains"); the others are applied here.
func listTopics(ctx context.Context, c *taba.Client, m *topicsModel) diag.Diagnostics {
	var diags diag.Diagnostics
	all, err := c.ListAllTopics(ctx, m.NameContains.ValueString())
	if err != nil {
		apiError(&diags, "Listing the topics", err)
		return diags
	}
	sort.Slice(all, func(i, j int) bool { return all[i].Name < all[j].Name })
	wantTags := stringMap(m.Tags)
	m.Topics = []topicModel{}
	names := []attr.Value{}
	for i := range all {
		t := &all[i]
		if t.DeadLetter != nil && *t.DeadLetter && !m.IncludeDeadLetters.ValueBool() {
			continue
		}
		if !m.Type.IsNull() && t.Type != m.Type.ValueString() {
			continue
		}
		if !hasTags(t.Tags, wantTags) {
			continue
		}
		m.Topics = append(m.Topics, topicFromAPI(t))
		names = append(names, types.StringValue(t.Name))
	}
	m.Names = types.ListValueMust(types.StringType, names)
	return diags
}

func hasTags(have, want map[string]string) bool {
	for k, v := range want {
		if got, ok := have[k]; !ok || got != v {
			return false
		}
	}
	return true
}
