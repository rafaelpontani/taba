package provider

import (
	"context"
	"fmt"
	"sort"

	"github.com/hashicorp/terraform-plugin-framework/attr"
	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/datasource/schema"
	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/types"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

var (
	_ datasource.DataSourceWithConfigure = (*tokenDataSource)(nil)
	_ datasource.DataSourceWithConfigure = (*tokensDataSource)(nil)
)

// tokenInfo is a token as the data sources show it: never the secret.
type tokenInfo struct {
	ID            types.String `tfsdk:"id"`
	Name          types.String `tfsdk:"name"`
	Permissions   types.Set    `tfsdk:"permissions"`
	TopicPrefixes types.Set    `tfsdk:"topic_prefixes"`
	CreatedAt     types.String `tfsdk:"created_at"`
	ExpiresAt     types.String `tfsdk:"expires_at"`
}

func tokenInfoFrom(t *taba.Token) tokenInfo {
	return tokenInfo{
		ID:            types.StringValue(t.ID),
		Name:          types.StringValue(t.Name),
		Permissions:   setValue(t.Permissions),
		TopicPrefixes: setValue(t.TopicPrefixes),
		CreatedAt:     stringOrNull(t.CreatedAt),
		ExpiresAt:     stringOrNull(t.ExpiresAt),
	}
}

func tokenAttributes(idRequired bool) map[string]schema.Attribute {
	id := schema.StringAttribute{MarkdownDescription: "Identifier of the token.", Computed: true}
	if idRequired {
		id = schema.StringAttribute{MarkdownDescription: "Identifier of the token.", Required: true}
	}
	return map[string]schema.Attribute{
		"id":             id,
		"name":           schema.StringAttribute{MarkdownDescription: "Name (not unique).", Computed: true},
		"permissions":    schema.SetAttribute{MarkdownDescription: "Permissions.", ElementType: types.StringType, Computed: true},
		"topic_prefixes": schema.SetAttribute{MarkdownDescription: "Prefixes the token is limited to (empty: every topic).", ElementType: types.StringType, Computed: true},
		"created_at":     schema.StringAttribute{MarkdownDescription: "Creation date (RFC 3339).", Computed: true},
		"expires_at":     schema.StringAttribute{MarkdownDescription: "Deadline (RFC 3339), or null when the token does not expire.", Computed: true},
	}
}

// -- taba_token -------------------------------------------------------------------------------------------------

func newTokenDataSource() datasource.DataSource { return &tokenDataSource{} }

type tokenDataSource struct {
	client *taba.Client
}

func (d *tokenDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_token"
}

func (d *tokenDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Reads one token by id, without the secret (the server only shows it at the creation). Needs authentication on and the root token or an `admin` token.",
		Attributes:          tokenAttributes(true),
	}
}

func (d *tokenDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	d.client = configureDataSource(req, resp)
}

func (d *tokenDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	var m tokenInfo
	resp.Diagnostics.Append(req.Config.Get(ctx, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	t, err := d.client.GetToken(ctx, m.ID.ValueString())
	if err != nil {
		tokenError(&resp.Diagnostics, fmt.Sprintf("Reading the token %s", m.ID.ValueString()), err)
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, tokenInfoFrom(t))...)
}

// -- taba_tokens ------------------------------------------------------------------------------------------------

func newTokensDataSource() datasource.DataSource { return &tokensDataSource{} }

type tokensDataSource struct {
	client *taba.Client
}

type tokensModel struct {
	NameContains types.String `tfsdk:"name_contains"`
	IDs          types.List   `tfsdk:"ids"`
	Tokens       []tokenInfo  `tfsdk:"tokens"`
}

func (d *tokensDataSource) Metadata(_ context.Context, req datasource.MetadataRequest, resp *datasource.MetadataResponse) {
	resp.TypeName = req.ProviderTypeName + "_tokens"
}

func (d *tokensDataSource) Schema(_ context.Context, _ datasource.SchemaRequest, resp *datasource.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Lists the tokens, oldest first, without the secrets. Expired tokens are not listed. Needs authentication on and the root token or an `admin` token.",
		Attributes: map[string]schema.Attribute{
			"name_contains": schema.StringAttribute{
				MarkdownDescription: "Only the tokens whose name contains this text.",
				Optional:            true,
			},
			"ids": schema.ListAttribute{
				MarkdownDescription: "The ids of the tokens found.",
				ElementType:         types.StringType,
				Computed:            true,
			},
			"tokens": schema.ListNestedAttribute{
				MarkdownDescription: "The tokens found.",
				Computed:            true,
				NestedObject:        schema.NestedAttributeObject{Attributes: tokenAttributes(false)},
			},
		},
	}
}

func (d *tokensDataSource) Configure(_ context.Context, req datasource.ConfigureRequest, resp *datasource.ConfigureResponse) {
	d.client = configureDataSource(req, resp)
}

func (d *tokensDataSource) Read(ctx context.Context, req datasource.ReadRequest, resp *datasource.ReadResponse) {
	var m tokensModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(listTokens(ctx, d.client, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, m)...)
}

// listTokens fills ids and tokens of m, sorted by creation and then id (the server filters the name).
func listTokens(ctx context.Context, c *taba.Client, m *tokensModel) diag.Diagnostics {
	var diags diag.Diagnostics
	all, err := c.ListAllTokens(ctx, m.NameContains.ValueString())
	if err != nil {
		tokenError(&diags, "Listing the tokens", err)
		return diags
	}
	sort.SliceStable(all, func(i, j int) bool {
		if all[i].CreatedAt != all[j].CreatedAt {
			return all[i].CreatedAt < all[j].CreatedAt
		}
		return all[i].ID < all[j].ID
	})
	m.Tokens = []tokenInfo{}
	ids := []attr.Value{}
	for i := range all {
		m.Tokens = append(m.Tokens, tokenInfoFrom(&all[i]))
		ids = append(ids, types.StringValue(all[i].ID))
	}
	m.IDs = types.ListValueMust(types.StringType, ids)
	return diags
}
