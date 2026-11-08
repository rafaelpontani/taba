package taba

import (
	"regexp"
	"strings"

	"github.com/pulumi/pulumi-terraform-bridge/v3/pkg/tfbridge"
)

// The documentation of the schema comes from the Terraform docs (connectors/terraform/docs). Two pieces of it talk
// about Terraform only and are rewritten before the examples are converted: the lifecycle block of the token example
// (the converter cannot turn it into Pulumi code) and the sentence about rotating a token.
var lifecycleBlock = regexp.MustCompile(`(?s)\n\n  # Rotation:.*?\n  lifecycle \{.*?\n  \}\n`)

const (
	terraformRotation = "To rotate, `terraform apply -replace=<address>`, with `lifecycle { create_before_destroy = true }` so there is no moment without a token."
	pulumiRotation    = "To rotate, run `pulumi up --replace <urn of the token>`: the new token is created before the old one is revoked, so there is no moment without a token."
)

func docsEdits(defaults []tfbridge.DocsEdit) []tfbridge.DocsEdit {
	return append(defaults, tfbridge.DocsEdit{
		Path: "*",
		Edit: func(_ string, content []byte) ([]byte, error) {
			text := lifecycleBlock.ReplaceAllString(string(content), "\n")
			text = strings.ReplaceAll(text, terraformRotation, pulumiRotation)
			return []byte(text), nil
		},
	})
}
