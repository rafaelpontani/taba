# A token that publishes and consumes only the topics starting with "orders".
# The secret is in taba_token.orders_app.token (sensitive): hand it to the application
# through a secret store, and keep the Terraform state as a secret too.
resource "taba_token" "orders_app" {
  name           = "orders-app"
  permissions    = ["publish", "consume"]
  topic_prefixes = ["orders"]
  expires_at     = "2027-12-31T23:59:59Z"

  # Rotation: terraform apply -replace=taba_token.orders_app creates the new
  # token before revoking the old one, so there is no moment without a token.
  lifecycle {
    create_before_destroy = true
  }
}

output "orders_app_token" {
  value     = taba_token.orders_app.token
  sensitive = true
}
