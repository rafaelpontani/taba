# The queues of the billing team whose name contains "orders".
data "taba_topics" "billing_orders" {
  name_contains = "orders"
  type          = "queue"
  tags = {
    team = "billing"
  }
}

output "names" {
  value = data.taba_topics.billing_orders.names
}
