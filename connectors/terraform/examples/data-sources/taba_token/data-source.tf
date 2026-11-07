# Reads a token by id, without the secret.
data "taba_token" "ci" {
  id = "01JC8X3V5Q2W7E9R4T6Y8U0I1O"
}

output "ci_expires_at" {
  value = data.taba_token.ci.expires_at
}
