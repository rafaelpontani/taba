// Command terraform-provider-taba is the Terraform (and OpenTofu) provider of Taba (doc 40).
package main

import (
	"context"
	"flag"
	"log"

	"github.com/hashicorp/terraform-plugin-framework/providerserver"

	"github.com/rafaelpontani/taba/connectors/terraform/shim"
)

// version is written by the release build (-ldflags "-X main.version=...").
var version = "dev"

func main() {
	var debug bool
	flag.BoolVar(&debug, "debug", false, "start the provider in debug mode (for delve)")
	flag.Parse()

	err := providerserver.Serve(context.Background(), shim.New(version), providerserver.ServeOpts{
		Address: "registry.terraform.io/rafaelpontani/taba",
		Debug:   debug,
	})
	if err != nil {
		log.Fatal(err)
	}
}
