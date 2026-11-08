// Command pulumi-resource-taba is the Pulumi resource plugin of Taba (the bridged Terraform provider, doc 41).
package main

import (
	"context"
	// embed keeps the schema and the bridge metadata in the binary.
	_ "embed"

	"github.com/pulumi/pulumi-terraform-bridge/v3/pkg/pf/tfbridge"

	taba "github.com/rafaelpontani/taba/connectors/pulumi/provider"
)

//go:embed schema-embed.json
var schema []byte

//go:embed bridge-metadata.json
var bridgeMetadata []byte

func main() {
	meta := tfbridge.ProviderMetadata{PackageSchema: schema, BridgeMetadata: bridgeMetadata}
	tfbridge.Main(context.Background(), "taba", taba.Provider(), meta)
}
