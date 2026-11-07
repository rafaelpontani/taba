terraform {
  required_providers {
    taba = {
      source = "rafaelpontani/taba"
    }
  }
}

# The endpoint and the token can also come from TABA_URL and TABA_TOKEN.
provider "taba" {
  endpoint = "https://taba.example.com:8080"
  token    = var.taba_token
  # ca_cert_file = "ca.pem" # a private certificate authority

  # Take over the queues, distributors and tokens that already exist (created by hand, by taba-cli or by an
  # application at startup) instead of failing. Off by default; each resource can override it.
  # create_or_update = true
}

variable "taba_token" {
  type      = string
  sensitive = true
}
