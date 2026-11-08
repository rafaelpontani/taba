# A durable queue: what is not declared follows the node default (see the effective_* attributes).
resource "taba_queue" "orders" {
  name           = "orders"
  ttl            = 86400
  max_deliveries = 5
  tags = {
    team = "billing"
  }
}

# A FIFO queue (the cluster needs replication.factor of 2 or more).
resource "taba_queue" "payments" {
  name = "payments"
  fifo = true
}

# A volatile queue, removed after 15 minutes without a consumer. force_destroy lets a destroy remove it with
# messages in it.
resource "taba_queue" "notifications" {
  name          = "notifications"
  volatile      = true
  idle_timeout  = 900
  force_destroy = true
}

# A queue that may already exist (created by an application at startup, say): it is taken over and gets the
# declared configuration instead of failing the creation. From then on it belongs to this state, and a destroy
# removes it.
resource "taba_queue" "events" {
  name             = "events"
  ttl              = 3600
  create_or_update = true
}
