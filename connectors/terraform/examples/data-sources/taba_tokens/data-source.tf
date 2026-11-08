# The tokens whose name contains "ci", oldest first, without the secrets.
data "taba_tokens" "ci" {
  name_contains = "ci"
}

output "ci_token_ids" {
  value = data.taba_tokens.ci.ids
}
