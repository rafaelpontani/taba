# A queue another team created, and its dead-letter queue.
data "taba_topic" "orders" {
  name = "orders"
}

data "taba_topic" "orders_dead_letter" {
  name = "orders-dead-letter"
}

output "orders_ttl" {
  value = data.taba_topic.orders.effective_ttl
}
