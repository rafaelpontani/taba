// Package provider is the Terraform (and OpenTofu) provider of Taba: queues, distributors and, with authentication
// on, the access tokens, all through the client API (doc 40). It talks to the cluster with the Go connector
// (connectors/go), which already retries what the common contract says to retry.
package provider

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/hashicorp/terraform-plugin-framework-validators/stringvalidator"
	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/provider"
	"github.com/hashicorp/terraform-plugin-framework/provider/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/schema/validator"
	"github.com/hashicorp/terraform-plugin-framework/types"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

// Environment variables read when the attribute is not set in the configuration.
const (
	envURL            = "TABA_URL"
	envToken          = "TABA_TOKEN"
	envCACert         = "TABA_CA_CERT"
	envCreateOrUpdate = "TABA_CREATE_OR_UPDATE"
)

const (
	defaultRequestTimeout = 30 * time.Second
	// defaultRetryMaxWait: the total wait between the attempts of a call that gets 429 or 503. A node that crashed is
	// still a member, and the owner of some definitions, until the others vote it out (about a minute), and the server
	// asks to wait 5 s each time: three minutes cover it with room.
	defaultRetryMaxWait = 3 * time.Minute
	// retryAttempts only bounds a call whose waits are all zero; the budget is retry_max_wait.
	retryAttempts = 100
)

// New returns the constructor of the provider for the given version (the one the release writes in the binary).
func New(version string) func() provider.Provider {
	return func() provider.Provider { return &tabaProvider{version: version} }
}

type tabaProvider struct {
	version string
}

type providerModel struct {
	Endpoint       types.String `tfsdk:"endpoint"`
	Token          types.String `tfsdk:"token"`
	CACertPEM      types.String `tfsdk:"ca_cert_pem"`
	CACertFile     types.String `tfsdk:"ca_cert_file"`
	RequestTimeout types.String `tfsdk:"request_timeout"`
	RetryMaxWait   types.String `tfsdk:"retry_max_wait"`
	CreateOrUpdate types.Bool   `tfsdk:"create_or_update"`
}

// providerData is what the resources and data sources receive in Configure.
type providerData struct {
	client *taba.Client
	// createOrUpdate is the default of the resources that do not set create_or_update.
	createOrUpdate bool
}

func (p *tabaProvider) Metadata(_ context.Context, _ provider.MetadataRequest, resp *provider.MetadataResponse) {
	resp.TypeName = "taba"
	resp.Version = p.version
}

func (p *tabaProvider) Schema(_ context.Context, _ provider.SchemaRequest, resp *provider.SchemaResponse) {
	resp.Schema = schema.Schema{
		MarkdownDescription: "Manages the queues, distributors and access tokens of a [Taba](https://github.com/rafaelpontani/taba) cluster through its client API.",
		Attributes: map[string]schema.Attribute{
			"endpoint": schema.StringAttribute{
				MarkdownDescription: "URL of the client API (the cluster load balancer), such as `https://taba.example.com:8080`. Default: the `TABA_URL` environment variable.",
				Optional:            true,
				Validators: []validator.String{
					stringvalidator.RegexMatches(urlPattern, "must start with http:// or https://"),
				},
			},
			"token": schema.StringAttribute{
				MarkdownDescription: "Access token (`Authorization: Bearer`). Queues and distributors need `create` and `manage` within their prefixes; tokens need the root token (or an `admin` one for tokens that are not `admin`). Default: the `TABA_TOKEN` environment variable. Without it, the calls go without authentication.",
				Optional:            true,
				Sensitive:           true,
			},
			"ca_cert_pem": schema.StringAttribute{
				MarkdownDescription: "PEM of a private certificate authority that signs the API certificate. Conflicts with `ca_cert_file`. Default: the `TABA_CA_CERT` environment variable (a PEM or a file path).",
				Optional:            true,
				Validators: []validator.String{
					stringvalidator.ConflictsWith(path.MatchRoot("ca_cert_file")),
				},
			},
			"ca_cert_file": schema.StringAttribute{
				MarkdownDescription: "Path of a PEM file with a private certificate authority. Conflicts with `ca_cert_pem`.",
				Optional:            true,
			},
			"request_timeout": schema.StringAttribute{
				MarkdownDescription: "Timeout of each call, as a Go duration (`30s`, `1m`). Default: `30s`.",
				Optional:            true,
			},
			"retry_max_wait": schema.StringAttribute{
				MarkdownDescription: "Total wait, as a Go duration (`3m`, `90s`), between the attempts of a call that gets `429` or `503`: each one waits for the `Retry-After` (5 s from a node of the cluster) or 1, 2, 4… up to 64 seconds, and no attempt is made whose wait would pass this total. A node that crashed answers for the definitions it owns until the others vote it out (about a minute). It also bounds the creations of a token that get `503 cluster.quorum-unavailable`, each one after revoking what the previous one may have created. `0s`: no new attempt. Default: `3m`.",
				Optional:            true,
			},
			"create_or_update": schema.BoolAttribute{
				MarkdownDescription: "Default of the `create_or_update` attribute of every resource: when creating a queue, distributor or token that already exists in the cluster, take it over (and bring it to the declared configuration) instead of failing. Off by default, because it takes over in silence what may belong to another configuration. Default: the `TABA_CREATE_OR_UPDATE` environment variable (`true` or `false`), or `false`.",
				Optional:            true,
			},
		},
	}
}

func (p *tabaProvider) Configure(ctx context.Context, req provider.ConfigureRequest, resp *provider.ConfigureResponse) {
	var m providerModel
	resp.Diagnostics.Append(req.Config.Get(ctx, &m)...)
	if resp.Diagnostics.HasError() {
		return
	}
	config, err := clientConfig(m, p.version, os.Getenv)
	if err != nil {
		resp.Diagnostics.AddError("Invalid provider configuration", err.Error())
		return
	}
	client, err := taba.NewClient(config)
	if err != nil {
		resp.Diagnostics.AddError("Invalid provider configuration", err.Error())
		return
	}
	createOrUpdate, err := createOrUpdateDefault(m, os.Getenv)
	if err != nil {
		resp.Diagnostics.AddError("Invalid provider configuration", err.Error())
		return
	}
	data := &providerData{client: client, createOrUpdate: createOrUpdate}
	resp.ResourceData = data
	resp.DataSourceData = data
}

func (p *tabaProvider) Resources(_ context.Context) []func() resource.Resource {
	return []func() resource.Resource{
		newQueueResource,
		newDistributorResource,
		newTokenResource,
	}
}

func (p *tabaProvider) DataSources(_ context.Context) []func() datasource.DataSource {
	return []func() datasource.DataSource{
		newTopicDataSource,
		newTopicsDataSource,
		newTokenDataSource,
		newTokensDataSource,
	}
}

// clientConfig turns the provider configuration (and the environment, through getenv) into the connector one.
// An unknown value (a variable known only at apply) is an error: the provider has to talk to the cluster to plan.
func clientConfig(m providerModel, version string, getenv func(string) string) (taba.Config, error) {
	for name, v := range map[string]interface{ IsUnknown() bool }{
		"endpoint": m.Endpoint, "token": m.Token, "ca_cert_pem": m.CACertPEM, "ca_cert_file": m.CACertFile,
		"request_timeout": m.RequestTimeout, "retry_max_wait": m.RetryMaxWait,
	} {
		if v.IsUnknown() {
			return taba.Config{}, fmt.Errorf("%s is only known after apply; the provider needs it to plan", name)
		}
	}
	config := taba.Config{
		BaseURL:        valueOr(m.Endpoint, getenv(envURL)),
		Token:          valueOr(m.Token, getenv(envToken)),
		RequestTimeout: defaultRequestTimeout,
		MaxAttempts:    retryAttempts,
		RetryMaxWait:   defaultRetryMaxWait,
		UserAgent:      "terraform-provider-taba/" + version,
	}
	if config.BaseURL == "" {
		return taba.Config{}, fmt.Errorf("endpoint is required (or the %s environment variable)", envURL)
	}
	if !urlPattern.MatchString(config.BaseURL) {
		return taba.Config{}, fmt.Errorf("endpoint %q must start with http:// or https://", config.BaseURL)
	}
	if !m.RequestTimeout.IsNull() {
		d, err := time.ParseDuration(m.RequestTimeout.ValueString())
		if err != nil || d <= 0 {
			return taba.Config{}, fmt.Errorf("request_timeout %q is not a positive duration (30s, 1m)", m.RequestTimeout.ValueString())
		}
		config.RequestTimeout = d
	}
	if !m.RetryMaxWait.IsNull() {
		d, err := time.ParseDuration(m.RetryMaxWait.ValueString())
		if err != nil || d < 0 {
			return taba.Config{}, fmt.Errorf("retry_max_wait %q is not a duration (3m, 90s, 0s)", m.RetryMaxWait.ValueString())
		}
		config.RetryMaxWait = d
		if d == 0 {
			config.MaxAttempts = 1
		}
	}
	pem, err := caPEM(m, getenv)
	if err != nil {
		return taba.Config{}, err
	}
	if pem != "" {
		roots := x509.NewCertPool()
		if !roots.AppendCertsFromPEM([]byte(pem)) {
			return taba.Config{}, fmt.Errorf("the certificate authority has no PEM certificate")
		}
		config.TLSConfig = &tls.Config{RootCAs: roots, MinVersion: tls.VersionTLS12}
	}
	return config, nil
}

// caPEM is the PEM of the private authority: ca_cert_pem, the file of ca_cert_file, or TABA_CA_CERT (a PEM, or the
// path of one).
func caPEM(m providerModel, getenv func(string) string) (string, error) {
	if !m.CACertPEM.IsNull() {
		return m.CACertPEM.ValueString(), nil
	}
	file := ""
	if !m.CACertFile.IsNull() {
		file = m.CACertFile.ValueString()
	} else if env := strings.TrimSpace(getenv(envCACert)); env != "" {
		if strings.HasPrefix(env, "-----BEGIN") {
			return env, nil
		}
		file = env
	}
	if file == "" {
		return "", nil
	}
	data, err := os.ReadFile(file)
	if err != nil {
		return "", fmt.Errorf("reading the certificate authority: %v", err)
	}
	return string(data), nil
}

// createOrUpdateDefault is create_or_update of the provider, or TABA_CREATE_OR_UPDATE, or false.
func createOrUpdateDefault(m providerModel, getenv func(string) string) (bool, error) {
	if m.CreateOrUpdate.IsUnknown() {
		return false, fmt.Errorf("create_or_update is only known after apply; the provider needs it to plan")
	}
	if !m.CreateOrUpdate.IsNull() {
		return m.CreateOrUpdate.ValueBool(), nil
	}
	env := strings.TrimSpace(getenv(envCreateOrUpdate))
	if env == "" {
		return false, nil
	}
	on, err := strconv.ParseBool(env)
	if err != nil {
		return false, fmt.Errorf("%s=%q is not true or false", envCreateOrUpdate, env)
	}
	return on, nil
}

func valueOr(v types.String, fallback string) string {
	if v.IsNull() {
		return fallback
	}
	return v.ValueString()
}
