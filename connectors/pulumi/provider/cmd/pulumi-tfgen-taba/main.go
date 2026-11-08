// Command pulumi-tfgen-taba generates the Pulumi schema and the SDKs from the Terraform provider (scripts/generate.sh).
package main

import (
	"github.com/pulumi/pulumi-terraform-bridge/v3/pkg/pf/tfgen"

	taba "github.com/rafaelpontani/taba/connectors/pulumi/provider"
)

func main() {
	tfgen.Main("taba", taba.Provider())
}
