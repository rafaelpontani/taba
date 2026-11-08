// Package taba maps the Terraform provider of Taba (connectors/terraform, doc 40) to a Pulumi provider through the
// pulumi-terraform-bridge (Plugin Framework). The behaviour lives in the Terraform provider; this file only names the
// Pulumi tokens and the metadata. No resource has autonaming: the name of a queue, distributor or token is declared.
package taba

import (
	// embed keeps bridge-metadata.json in the binaries.
	_ "embed"

	pf "github.com/pulumi/pulumi-terraform-bridge/v3/pkg/pf/tfbridge"
	"github.com/pulumi/pulumi-terraform-bridge/v3/pkg/tfbridge"
	"github.com/pulumi/pulumi-terraform-bridge/v3/pkg/tfbridge/tokens"

	"github.com/rafaelpontani/taba/connectors/pulumi/provider/pkg/version"
	"github.com/rafaelpontani/taba/connectors/terraform/shim"
)

const (
	pkgName = "taba"
	modName = "index"
)

//go:embed cmd/pulumi-resource-taba/bridge-metadata.json
var metadata []byte

var unlicensed = tfbridge.UnlicensedLicenseType

// Provider returns the bridged provider and its metadata.
func Provider() tfbridge.ProviderInfo {
	prov := tfbridge.ProviderInfo{
		P:           pf.ShimProvider(shim.New(version.Version)()),
		Name:        pkgName,
		DisplayName: "Taba",
		Publisher:   "rafaelpontani",
		Description: "Queues, distributors and access tokens of the Taba messaging system.",
		Keywords:    []string{"pulumi", pkgName, "messaging", "queue", "category/infrastructure"},
		License:     "Apache-2.0",
		// The bridge writes a README that calls the SDK "a derived work of the Terraform provider, under MPL 2.0" and sends
		// bug reports to terraform-provider-taba. Neither is true here (the Terraform provider is ours, Apache-2.0, and
		// the same code), so it writes none and package.sh puts connectors/pulumi/README.md in the packages.
		TFProviderLicense: &unlicensed,
		Homepage:          "https://github.com/rafaelpontani/taba",
		Repository:        "https://github.com/rafaelpontani/pulumi-taba",
		GitHubOrg:         "rafaelpontani",
		PluginDownloadURL: "github://api.github.com/rafaelpontani/pulumi-taba",
		Version:           version.Version,
		MetadataInfo:      tfbridge.NewProviderMetadata(metadata),
		UpstreamRepoPath:  "../../terraform",
		Resources: map[string]*tfbridge.ResourceInfo{
			// The name is the id: a new queue or distributor with the same name only exists after the old one is gone.
			"taba_queue":       {DeleteBeforeReplace: true},
			"taba_distributor": {DeleteBeforeReplace: true},
			// The C# class Token cannot have a property Token (CS0542), so the secret is named secret in every language.
			"taba_token": {Fields: map[string]*tfbridge.SchemaInfo{"token": {Name: "secret"}}},
		},
		JavaScript: &tfbridge.JavaScriptInfo{
			PackageName: "@rafaelpontani/pulumi-taba",
		},
		Python: func() *tfbridge.PythonInfo {
			i := &tfbridge.PythonInfo{PackageName: "pulumi_taba"}
			i.PyProject.Enabled = true
			return i
		}(),
		Golang: &tfbridge.GolangInfo{
			ImportBasePath:                 "github.com/rafaelpontani/pulumi-taba/sdk/go/taba",
			GenerateResourceContainerTypes: true,
		},
		CSharp: &tfbridge.CSharpInfo{
			RootNamespace:     "Taba",
			Namespaces:        map[string]string{pkgName: "Pulumi"},
			PackageReferences: map[string]string{"Pulumi": "3.*"},
		},
		EnableAccurateBridgePreview: true,
		DocRules:                    &tfbridge.DocRuleInfo{EditRules: docsEdits},
	}
	prov.MustComputeTokens(tokens.SingleModule("taba_", modName, tokens.MakeStandard(pkgName)))
	prov.MustApplyAutoAliases()
	return prov
}
