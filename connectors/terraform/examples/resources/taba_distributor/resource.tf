# Each publication to "order-events" is copied to every queue whose name starts with "orders-" or "audit-".
resource "taba_distributor" "order_events" {
  name     = "order-events"
  prefixes = ["orders-", "audit-"]
}
