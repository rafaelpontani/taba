package provider

// Acceptance tests: real Terraform (or OpenTofu, through TF_ACC_TERRAFORM_PATH) against a real node. They only run
// with TF_ACC=1 and the node in TABA_URL, with the root token in TABA_TOKEN (scripts/test.sh starts one node with
// authentication on and runs them). After every step the framework plans again and fails when the plan is not
// empty, so each step also checks that what was applied reads back without a difference.

import (
	"context"
	"fmt"
	"os"
	"regexp"
	"testing"
	"time"

	"github.com/hashicorp/terraform-plugin-framework/providerserver"
	"github.com/hashicorp/terraform-plugin-go/tfprotov6"
	"github.com/hashicorp/terraform-plugin-testing/helper/resource"
	"github.com/hashicorp/terraform-plugin-testing/terraform"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

var accProviders = map[string]func() (tfprotov6.ProviderServer, error){
	"taba": providerserver.NewProtocol6WithError(New("acc")()),
}

// accOnly skips the test without TF_ACC, before anything of it (cleanups, checks after resource.Test) calls the node.
func accOnly(t *testing.T) {
	t.Helper()
	if os.Getenv("TF_ACC") == "" {
		t.Skip("acceptance test: set TF_ACC=1 (scripts/test.sh)")
	}
}

// accPreCheck fails early without a node.
func accPreCheck(t *testing.T) {
	t.Helper()
	if os.Getenv(envURL) == "" || os.Getenv(envToken) == "" {
		t.Fatalf("the acceptance tests need %s and %s (the root token); scripts/test.sh sets them", envURL, envToken)
	}
}

// accClient is a connector client of the same node, to prepare (or break) what the steps find.
func accClient(t *testing.T) *taba.Client {
	t.Helper()
	c, err := taba.NewClient(taba.Config{BaseURL: os.Getenv(envURL), Token: os.Getenv(envToken), MaxAttempts: 100, RetryMaxWait: 3 * time.Minute})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(c.Close)
	return c
}

// accName is a topic or token name unique to this run, so runs against the same node do not meet.
func accName(t *testing.T, base string) string {
	t.Helper()
	return fmt.Sprintf("acc-%s-%d", base, time.Now().UnixNano()%1_000_000_000)
}

func accContext(t *testing.T) context.Context {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	t.Cleanup(cancel)
	return ctx
}

// accTopicsGone is the CheckDestroy of the topic resources: nothing of the state is left in the cluster.
func accTopicsGone(t *testing.T) resource.TestCheckFunc {
	return func(s *terraform.State) error {
		c := accClient(t)
		for _, rs := range s.RootModule().Resources {
			if rs.Type != "taba_queue" && rs.Type != "taba_distributor" {
				continue
			}
			_, err := c.GetTopic(accContext(t), rs.Primary.Attributes["name"])
			if errorCode(err) != "topic.not-found" {
				return fmt.Errorf("%s still exists after the destroy (%v)", rs.Primary.Attributes["name"], err)
			}
		}
		return nil
	}
}

// accTokensGone checks that every token of the state was revoked.
func accTokensGone(t *testing.T) resource.TestCheckFunc {
	return func(s *terraform.State) error {
		c := accClient(t)
		for _, rs := range s.RootModule().Resources {
			if rs.Type != "taba_token" {
				continue
			}
			_, err := c.GetToken(accContext(t), rs.Primary.ID)
			if errorCode(err) != "token.not-found" {
				return fmt.Errorf("the token %s still exists after the destroy (%v)", rs.Primary.ID, err)
			}
		}
		return nil
	}
}

// accSameValue keeps the value of an attribute in one step and compares it in a later one (same or another id).
type accSameValue struct{ value string }

func (v *accSameValue) keep(address, attribute string) resource.TestCheckFunc {
	return resource.TestCheckResourceAttrWith(address, attribute, func(value string) error {
		v.value = value
		return nil
	})
}

func (v *accSameValue) equal(address, attribute string, want bool) resource.TestCheckFunc {
	return resource.TestCheckResourceAttrWith(address, attribute, func(value string) error {
		if (value == v.value) != want {
			if want {
				return fmt.Errorf("%s.%s changed from %q to %q", address, attribute, v.value, value)
			}
			return fmt.Errorf("%s.%s kept %q, and it should have changed", address, attribute, value)
		}
		return nil
	})
}

// UC: UC-IAC-002, UC-IAC-004, UC-IAC-005, UC-IAC-007
func TestAccQueue(t *testing.T) {
	accOnly(t)
	name := accName(t, "queue")
	id := &accSameValue{}
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTopicsGone(t),
		Steps: []resource.TestStep{
			{
				Config: fmt.Sprintf(`
resource "taba_queue" "q" {
  name           = %q
  ttl            = 3600
  max_deliveries = 4
  tags           = { team = "billing" }
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					id.keep("taba_queue.q", "id"),
					resource.TestCheckResourceAttr("taba_queue.q", "effective_ttl", "3600"),
					resource.TestCheckResourceAttr("taba_queue.q", "effective_max_deliveries", "4"),
					resource.TestCheckResourceAttrSet("taba_queue.q", "effective_max_message_bytes"),
					resource.TestCheckResourceAttr("taba_queue.q", "dead_letter_name", name+"-dead-letter"),
					resource.TestCheckResourceAttr("taba_queue.q", "tags.team", "billing"),
					resource.TestCheckNoResourceAttr("taba_queue.q", "idle_timeout"),
				),
			},
			{
				ResourceName:            "taba_queue.q",
				ImportState:             true,
				ImportStateId:           name,
				ImportStateVerify:       true,
				ImportStateVerifyIgnore: []string{"force_destroy", "create_or_update"},
			},
			{
				// ttl and max_deliveries leave the configuration: they go back to the node default, in place.
				Config: fmt.Sprintf(`
resource "taba_queue" "q" {
  name = %q
  tags = { team = "payments", tier = "gold" }
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					id.equal("taba_queue.q", "id", true),
					resource.TestCheckNoResourceAttr("taba_queue.q", "ttl"),
					resource.TestCheckNoResourceAttr("taba_queue.q", "max_deliveries"),
					resource.TestCheckResourceAttrWith("taba_queue.q", "effective_ttl", func(v string) error {
						if v == "3600" {
							return fmt.Errorf("effective_ttl is still 3600")
						}
						return nil
					}),
					resource.TestCheckResourceAttr("taba_queue.q", "tags.%", "2"),
				),
			},
			{
				// volatile does not change in place: the queue is replaced (an empty one, so no force_destroy).
				Config: fmt.Sprintf(`
resource "taba_queue" "q" {
  name         = %q
  volatile     = true
  idle_timeout = 900
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					id.equal("taba_queue.q", "id", false),
					resource.TestCheckResourceAttr("taba_queue.q", "volatile", "true"),
					resource.TestCheckResourceAttr("taba_queue.q", "idle_timeout", "900"),
				),
			},
			{
				// Without idle_timeout a volatile queue goes back to the node default (600).
				Config: fmt.Sprintf(`
resource "taba_queue" "q" {
  name     = %q
  volatile = true
}`, name),
				Check: resource.TestCheckResourceAttr("taba_queue.q", "idle_timeout", "600"),
			},
		},
	})
}

// UC: UC-IAC-006
func TestAccQueueDrift(t *testing.T) {
	accOnly(t)
	name := accName(t, "drift")
	config := fmt.Sprintf(`
resource "taba_queue" "q" {
  name = %q
  ttl  = 120
}`, name)
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTopicsGone(t),
		Steps: []resource.TestStep{
			{Config: config},
			{
				// Changed outside Terraform: the plan brings it back.
				PreConfig: func() {
					c := accClient(t)
					if _, err := c.UpdateTopic(accContext(t), name, &taba.TopicUpdate{TTL: taba.SetInt(999)}); err != nil {
						t.Fatal(err)
					}
				},
				Config:             config,
				PlanOnly:           true,
				ExpectNonEmptyPlan: true,
			},
			{Config: config, Check: resource.TestCheckResourceAttr("taba_queue.q", "effective_ttl", "120")},
			{
				// Removed outside Terraform: the plan creates it again.
				PreConfig: func() {
					if err := accClient(t).DeleteTopic(accContext(t), name); err != nil {
						t.Fatal(err)
					}
				},
				Config:             config,
				PlanOnly:           true,
				ExpectNonEmptyPlan: true,
			},
			{Config: config, Check: resource.TestCheckResourceAttr("taba_queue.q", "ttl", "120")},
		},
	})
}

// UC: UC-IAC-008
func TestAccQueueForceDestroy(t *testing.T) {
	accOnly(t)
	name := accName(t, "force")
	queue := func(volatile, force bool) string {
		return fmt.Sprintf(`
resource "taba_queue" "q" {
  name          = %q
  volatile      = %t
  force_destroy = %t
}`, name, volatile, force)
	}
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTopicsGone(t),
		Steps: []resource.TestStep{
			{Config: queue(false, false)},
			{
				// A queue with a message is not replaced (nor removed) without force_destroy.
				PreConfig: func() {
					if _, err := accClient(t).PublishText(accContext(t), name, "kept", nil); err != nil {
						t.Fatal(err)
					}
				},
				Config:      queue(true, false),
				ExpectError: regexp.MustCompile(`still holds\s+messages`),
			},
			{
				// force_destroy comes from the state: applied first, in place...
				Config: queue(false, true),
				Check:  resource.TestCheckResourceAttr("taba_queue.q", "force_destroy", "true"),
			},
			{
				// ...and then the replacement removes the queue with its message.
				Config: queue(true, true),
				Check:  resource.TestCheckResourceAttr("taba_queue.q", "volatile", "true"),
			},
		},
	})
}

// UC: UC-IAC-011
func TestAccDistributor(t *testing.T) {
	accOnly(t)
	name := accName(t, "dist")
	id := &accSameValue{}
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTopicsGone(t),
		Steps: []resource.TestStep{
			{
				Config: fmt.Sprintf(`
resource "taba_distributor" "d" {
  name     = %q
  prefixes = ["orders-", "audit-"]
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					id.keep("taba_distributor.d", "id"),
					resource.TestCheckResourceAttr("taba_distributor.d", "prefixes.#", "2"),
					resource.TestCheckResourceAttr("taba_distributor.d", "prefixes.0", "orders-"),
				),
			},
			{
				ResourceName:            "taba_distributor.d",
				ImportState:             true,
				ImportStateId:           name,
				ImportStateVerify:       true,
				ImportStateVerifyIgnore: []string{"create_or_update"},
			},
			{
				Config: fmt.Sprintf(`
resource "taba_distributor" "d" {
  name     = %q
  prefixes = ["billing-"]
  tags     = { owner = "platform" }
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					id.equal("taba_distributor.d", "id", true),
					resource.TestCheckResourceAttr("taba_distributor.d", "prefixes.#", "1"),
					resource.TestCheckResourceAttr("taba_distributor.d", "tags.owner", "platform"),
				),
			},
		},
	})
}

// A queue imported as a distributor (or the other way) is an error, not a replacement.
// UC: UC-IAC-007
func TestAccImportWrongType(t *testing.T) {
	accOnly(t)
	name := accName(t, "wrong")
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTopicsGone(t),
		Steps: []resource.TestStep{
			{
				// The import step reuses this configuration (a step with its own one makes OpenTofu look the provider
				// up in its registry), so the distributor block is here, with another name.
				Config: fmt.Sprintf(`
resource "taba_queue" "q" {
  name = %q
}

resource "taba_distributor" "d" {
  name     = "%s-d"
  prefixes = ["x-"]
}`, name, name),
			},
			{
				ResourceName:  "taba_distributor.d",
				ImportState:   true,
				ImportStateId: name,
				ExpectError:   regexp.MustCompile(`(?i)queue`),
			},
		},
	})
}

// UC: UC-IAC-013, UC-IAC-014
func TestAccToken(t *testing.T) {
	accOnly(t)
	name := accName(t, "token")
	id := &accSameValue{}
	secret := &accSameValue{}
	expires := time.Now().Add(48 * time.Hour).UTC().Truncate(time.Second).Format(time.RFC3339)
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTokensGone(t),
		Steps: []resource.TestStep{
			{
				Config: fmt.Sprintf(`
resource "taba_token" "t" {
  name           = %q
  permissions    = ["publish", "consume"]
  topic_prefixes = ["orders-"]
  expires_at     = %q
}`, name, expires),
				Check: resource.ComposeAggregateTestCheckFunc(
					id.keep("taba_token.t", "id"),
					secret.keep("taba_token.t", "token"),
					resource.TestMatchResourceAttr("taba_token.t", "token", regexp.MustCompile(`^tb_`)),
					resource.TestCheckResourceAttr("taba_token.t", "permissions.#", "2"),
					resource.TestCheckResourceAttr("taba_token.t", "expires_at", expires),
				),
			},
			{
				// An import block with the same configuration plans nothing but the import: the server writes the
				// deadline as …:00.000Z and the declared …:00Z is the same instant, so it is no difference. No Config:
				// the step reuses the previous one (a step with its own makes OpenTofu look the provider up).
				ResourceName:    "taba_token.t",
				ImportState:     true,
				ImportStateKind: resource.ImportBlockWithID,
			},
			{
				// The name and the deadline change in place, and the secret stays.
				Config: fmt.Sprintf(`
resource "taba_token" "t" {
  name           = "%s-renamed"
  permissions    = ["publish", "consume"]
  topic_prefixes = ["orders-"]
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					id.equal("taba_token.t", "id", true),
					secret.equal("taba_token.t", "token", true),
					resource.TestCheckNoResourceAttr("taba_token.t", "expires_at"),
				),
			},
			{
				// Permissions do not change in place: a new token, with a new secret.
				Config: fmt.Sprintf(`
resource "taba_token" "t" {
  name           = "%s-renamed"
  permissions    = ["publish"]
  topic_prefixes = ["orders-"]
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					id.equal("taba_token.t", "id", false),
					secret.equal("taba_token.t", "token", false),
				),
			},
			{
				// Revoked outside Terraform: the plan creates it again.
				PreConfig: func() {
					c := accClient(t)
					tokens, err := c.ListAllTokens(accContext(t), name+"-renamed")
					if err != nil || len(tokens) != 1 {
						t.Fatalf("expected one token %s-renamed: %v %v", name, tokens, err)
					}
					if err := c.RevokeToken(accContext(t), tokens[0].ID); err != nil {
						t.Fatal(err)
					}
				},
				Config: fmt.Sprintf(`
resource "taba_token" "t" {
  name           = "%s-renamed"
  permissions    = ["publish"]
  topic_prefixes = ["orders-"]
}`, name),
				PlanOnly:           true,
				ExpectNonEmptyPlan: true,
			},
		},
	})
}

// UC: UC-IAC-015
func TestAccTokenPastDeadlineIsRefusedAtPlan(t *testing.T) {
	accOnly(t)
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		Steps: []resource.TestStep{
			{
				Config: `
resource "taba_token" "t" {
  name        = "acc-expired"
  permissions = ["consume"]
  expires_at  = "2020-01-01T00:00:00Z"
}`,
				PlanOnly:    true,
				ExpectError: regexp.MustCompile(`(?i)expired`),
			},
		},
	})
}

// UC: UC-IAC-019, UC-IAC-020
func TestAccCreateOrUpdateTopics(t *testing.T) {
	accOnly(t)
	queue := accName(t, "adopt")
	full := accName(t, "full")
	taken := accName(t, "taken")
	setup := func() {
		c := accClient(t)
		ctx := accContext(t)
		for _, request := range []*taba.TopicRequest{
			{Name: queue, Type: "queue", TTL: taba.Int(60), MaxDeliveries: taba.Int(3), Tags: map[string]string{"by": "hand"}},
			{Name: full, Type: "queue"},
			{Name: taken, Type: "queue"},
		} {
			if _, err := c.CreateTopic(ctx, request); err != nil {
				t.Fatal(err)
			}
		}
		if _, err := c.PublishText(ctx, full, "kept", nil); err != nil {
			t.Fatal(err)
		}
	}
	t.Cleanup(func() {
		c := accClient(t)
		for _, name := range []string{full, taken} {
			_ = c.DeleteTopic(accContext(t), name)
		}
	})
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTopicsGone(t),
		Steps: []resource.TestStep{
			{
				// Taken over: the declared configuration, and what is not declared goes back to the node default.
				PreConfig: setup,
				Config: fmt.Sprintf(`
resource "taba_queue" "q" {
  name             = %q
  ttl              = 3600
  create_or_update = true
}`, queue),
				Check: resource.ComposeAggregateTestCheckFunc(
					resource.TestCheckResourceAttr("taba_queue.q", "effective_ttl", "3600"),
					resource.TestCheckNoResourceAttr("taba_queue.q", "max_deliveries"),
					resource.TestCheckResourceAttr("taba_queue.q", "tags.%", "0"),
				),
			},
			{
				// Another volatile and a message in it: nothing changes, the error says why.
				Config: fmt.Sprintf(`
resource "taba_queue" "full" {
  name             = %q
  volatile         = true
  create_or_update = true
}`, full),
				ExpectError: regexp.MustCompile(`not\s+empty`),
			},
			{
				// Off: the creation fails with both ways out.
				Config: fmt.Sprintf(`
resource "taba_queue" "taken" {
  name = %q
}`, taken),
				ExpectError: regexp.MustCompile(`create_or_update\s+=\s+true`),
			},
		},
	})
	if topic, err := accClient(t).GetTopic(accContext(t), full); err != nil || (topic.Volatile != nil && *topic.Volatile) {
		t.Fatalf("the queue with a message must stay as it was: %+v %v", topic, err)
	}
}

// UC: UC-IAC-021
func TestAccCreateOrUpdateToken(t *testing.T) {
	accOnly(t)
	name := accName(t, "adopt-token")
	id := ""
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTokensGone(t),
		Steps: []resource.TestStep{
			{
				PreConfig: func() {
					created, err := accClient(t).CreateToken(accContext(t), &taba.TokenRequest{
						Name: name, Permissions: []string{"consume"}, TopicPrefixes: []string{"orders-"},
					})
					if err != nil {
						t.Fatal(err)
					}
					id = created.ID
				},
				Config: fmt.Sprintf(`
provider "taba" {
  create_or_update = true
}

resource "taba_token" "t" {
  name           = %q
  permissions    = ["consume"]
  topic_prefixes = ["orders-"]
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					resource.TestCheckResourceAttrWith("taba_token.t", "id", func(v string) error {
						if v != id {
							return fmt.Errorf("took %s, expected the existing %s", v, id)
						}
						return nil
					}),
					resource.TestCheckNoResourceAttr("taba_token.t", "token"),
				),
			},
		},
	})
}

// UC: UC-IAC-012
func TestAccDataSources(t *testing.T) {
	accOnly(t)
	name := accName(t, "ds")
	resource.Test(t, resource.TestCase{
		PreCheck:                 func() { accPreCheck(t) },
		ProtoV6ProviderFactories: accProviders,
		CheckDestroy:             accTopicsGone(t),
		Steps: []resource.TestStep{
			{
				Config: fmt.Sprintf(`
resource "taba_queue" "q" {
  name = %[1]q
  ttl  = 300
  tags = { team = "acc" }
}

resource "taba_token" "t" {
  name        = %[1]q
  permissions = ["consume"]
}

data "taba_topic" "q" {
  name = taba_queue.q.name
}

data "taba_topic" "dead_letter" {
  name = taba_queue.q.dead_letter_name
}

data "taba_topics" "mine" {
  name_contains = taba_queue.q.name
  tags          = { team = "acc" }
}

data "taba_token" "t" {
  id = taba_token.t.id
}

data "taba_tokens" "mine" {
  name_contains = taba_token.t.name
}`, name),
				Check: resource.ComposeAggregateTestCheckFunc(
					resource.TestCheckResourceAttr("data.taba_topic.q", "type", "queue"),
					resource.TestCheckResourceAttr("data.taba_topic.q", "effective_ttl", "300"),
					resource.TestCheckResourceAttr("data.taba_topic.dead_letter", "dead_letter", "true"),
					resource.TestCheckResourceAttr("data.taba_topics.mine", "names.#", "1"),
					resource.TestCheckResourceAttr("data.taba_topics.mine", "names.0", name),
					resource.TestCheckResourceAttrPair("data.taba_token.t", "name", "taba_token.t", "name"),
					resource.TestCheckResourceAttr("data.taba_tokens.mine", "ids.#", "1"),
				),
			},
		},
	})
}
