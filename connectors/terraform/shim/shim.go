// Package shim exposes the provider outside internal/, for the Pulumi bridge (which builds the Pulumi provider from
// this one) and for main.
package shim

import (
	"github.com/hashicorp/terraform-plugin-framework/provider"

	internal "github.com/rafaelpontani/taba/connectors/terraform/internal/provider"
)

// New returns the constructor of the provider for the given version.
func New(version string) func() provider.Provider {
	return internal.New(version)
}
