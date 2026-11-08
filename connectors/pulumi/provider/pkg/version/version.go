// Package version holds the version of the Pulumi provider, written by the release build
// (-ldflags "-X github.com/rafaelpontani/taba/connectors/pulumi/provider/pkg/version.Version=...").
package version

// Version of the provider and of the Terraform provider it embeds (the same version of the repository).
var Version = "0.0.0-dev"
