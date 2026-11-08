// The Go driver of the connector contract tests (connectors/contract/README.md): reads the scenario files and runs
// each line against a real Taba through the connector, in the style of the language.
//
//	go run ./contract --url http://host:8080 --token <token> --prefix ct-go-0101 --lang go \
//	    --scenario ../contract/scenarios/core.scn
package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/base64"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	taba "github.com/rafaelpontani/taba/connectors/go"
)

var args = map[string]string{}
var scenarios []string

func parseArgs(argv []string) {
	for i := 0; i+1 < len(argv); i += 2 {
		key := strings.TrimPrefix(argv[i], "--")
		if key == "scenario" {
			scenarios = append(scenarios, argv[i+1])
		} else {
			args[key] = argv[i+1]
		}
	}
}

// ---- the scenario language ------------------------------------------------------------------------------

var variablePattern = regexp.MustCompile(`\{p\}|\{lang\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)`)

// unquote decodes every %XX run (UTF-8 bytes); the rest stays as it is (a + is a +).
func unquote(s string) string {
	var out []byte
	for i := 0; i < len(s); i++ {
		if s[i] == '%' && i+2 < len(s) {
			if n, err := strconv.ParseUint(s[i+1:i+3], 16, 8); err == nil {
				out = append(out, byte(n))
				i += 2
				continue
			}
		}
		out = append(out, s[i])
	}
	return string(out)
}

type result map[string]string

// expand: literal parts are percent-decoded; {p}, {lang} and $name.field are replaced afterwards, untouched.
func expand(value string, variables map[string]result) string {
	var out bytes.Buffer
	last := 0
	for _, m := range variablePattern.FindAllStringSubmatchIndex(value, -1) {
		out.WriteString(unquote(value[last:m[0]]))
		switch whole := value[m[0]:m[1]]; whole {
		case "{p}":
			out.WriteString(args["prefix"])
		case "{lang}":
			out.WriteString(args["lang"])
		default:
			out.WriteString(variables[value[m[2]:m[3]]][value[m[4]:m[5]]])
		}
		last = m[1]
	}
	out.WriteString(unquote(value[last:]))
	return out.String()
}

type expectation struct{ key, op, value string }

func parseLine(text string, variables map[string]result) (string, map[string]string, []expectation) {
	head, tail := text, ""
	if i := strings.Index(text, " => "); i >= 0 {
		head, tail = text[:i], text[i+4:]
	}
	parts := strings.Fields(head)
	params := map[string]string{}
	for _, pair := range parts[1:] {
		key, value := pair, ""
		if i := strings.Index(pair, "="); i >= 0 {
			key, value = pair[:i], pair[i+1:]
		}
		params[key] = expand(value, variables)
	}
	var expects []expectation
	for _, token := range strings.Fields(tail) {
		i := strings.IndexAny(token, "=~")
		expects = append(expects, expectation{token[:i], token[i : i+1], expand(token[i+1:], variables)})
	}
	return parts[0], params, expects
}

// ---- helpers --------------------------------------------------------------------------------------------

func num(p map[string]string, key string) *int {
	v, ok := p[key]
	if !ok {
		return nil
	}
	n, _ := strconv.Atoi(v)
	return &n
}

func flag(p map[string]string, key string) *bool {
	v, ok := p[key]
	if !ok {
		return nil
	}
	b := v == "true"
	return &b
}

func list(p map[string]string, key string) []string {
	v, ok := p[key]
	if !ok {
		return nil
	}
	var out []string
	for _, x := range strings.Split(v, ",") {
		if x != "" {
			out = append(out, x)
		}
	}
	return out
}

func tags(p map[string]string, key string) map[string]string {
	v, ok := p[key]
	if !ok {
		return nil
	}
	out := map[string]string{}
	for _, t := range strings.Split(v, ",") {
		kv := strings.SplitN(t, ":", 2)
		out[kv[0]] = kv[1]
	}
	return out
}

// nullable: "null" goes back to the server default; absent leaves the field out.
func nullable(p map[string]string, key string) *taba.NullableInt {
	v, ok := p[key]
	if !ok {
		return nil
	}
	if v == "null" {
		return taba.ClearInt()
	}
	n, _ := strconv.Atoi(v)
	return taba.SetInt(n)
}

func render(m *taba.Message) string {
	media := strings.ToLower(strings.TrimSpace(strings.SplitN(m.ContentType, ";", 2)[0]))
	if media == "application/octet-stream" {
		return "b64:" + base64.StdEncoding.EncodeToString(m.Body)
	}
	return m.BodyText()
}

func textOfBool(b *bool) string {
	if b == nil {
		return ""
	}
	return strconv.FormatBool(*b)
}

func textOfInt(n *int) string {
	if n == nil {
		return ""
	}
	return strconv.Itoa(*n)
}

func topicFields(t *taba.Topic) result {
	return result{"id": t.ID, "name": t.Name, "type": t.Type, "fifo": textOfBool(t.Fifo), "volatile": textOfBool(t.Volatile), "raw": t.Raw}
}

func publishFields(r *taba.PublishResponse) result {
	return result{"id": r.ID, "duplicate": textOfBool(r.Duplicate), "matched": textOfInt(r.Matched)}
}

func consumeOptions(p map[string]string) *taba.ConsumeOptions {
	return &taba.ConsumeOptions{MaxMessages: num(p, "max"), WaitTime: num(p, "wait"), VisibilityTimeout: num(p, "visibility")}
}

func messageFields(messages []*taba.Message) result {
	bodies := make([]string, len(messages))
	for i, m := range messages {
		bodies[i] = render(m)
	}
	fields := result{"count": strconv.Itoa(len(messages)), "bodies": strings.Join(bodies, ",")}
	if len(messages) > 0 {
		first := messages[0]
		fields["id"] = first.ID
		fields["receiptToken"] = first.ReceiptToken
		fields["deliveryCount"] = strconv.Itoa(first.DeliveryCount)
		fields["contentType"] = first.ContentType
		fields["body"] = render(first)
	}
	return fields
}

func publishOptions(p map[string]string) *taba.PublishOptions {
	f := flag(p, "fifo")
	return &taba.PublishOptions{GroupID: p["groupId"], DeduplicationID: p["deduplicationId"], Fifo: f != nil && *f}
}

func statuses(results []taba.BatchResult) result {
	codes := make([]string, len(results))
	for i, r := range results {
		codes[i] = strconv.Itoa(r.Status)
	}
	return result{"count": strconv.Itoa(len(results)), "statuses": strings.Join(codes, ",")}
}

// ---- the operations -------------------------------------------------------------------------------------

type operation func(c *taba.Client, p map[string]string) (result, error)

var ctx = context.Background()

func topicRequest(p map[string]string) *taba.TopicRequest {
	return &taba.TopicRequest{
		Name: p["name"], Type: p["type"], Fifo: flag(p, "fifo"), Prefixes: list(p, "prefixes"), Tags: tags(p, "tags"),
		Volatile: flag(p, "volatile"), TTL: num(p, "ttl"), MaxDeliveries: num(p, "maxDeliveries"),
		MaxMessageBytes: num(p, "maxMessageBytes"), IdleTimeout: num(p, "idleTimeout"),
	}
}

func opCreateTopic(c *taba.Client, p map[string]string) (result, error) {
	t, err := c.CreateTopic(ctx, topicRequest(p))
	if err != nil {
		return nil, err
	}
	return topicFields(t), nil
}

func opCreateOrUpdateTopic(c *taba.Client, p map[string]string) (result, error) {
	put, err := c.CreateOrUpdateTopic(ctx, topicRequest(p), taba.Recreate(p["recreate"]))
	if err != nil {
		return nil, err
	}
	out := topicFields(&put.Topic)
	out["outcome"] = string(put.Outcome)
	return out, nil
}

func opListTopics(c *taba.Client, p map[string]string) (result, error) {
	topics, err := c.ListAllTopics(ctx, p["name"])
	if err != nil {
		return nil, err
	}
	names := make([]string, len(topics))
	for i, t := range topics {
		names[i] = t.Name
	}
	sort.Strings(names)
	return result{"count": strconv.Itoa(len(topics)), "names": strings.Join(names, ",")}, nil
}

func opUpdateTopic(c *taba.Client, p map[string]string) (result, error) {
	t, err := c.UpdateTopic(ctx, p["name"], &taba.TopicUpdate{
		Prefixes: list(p, "prefixes"), Tags: tags(p, "tags"), IdleTimeout: num(p, "idleTimeout"),
		TTL: nullable(p, "ttl"), MaxDeliveries: nullable(p, "maxDeliveries"), MaxMessageBytes: nullable(p, "maxMessageBytes"),
	})
	if err != nil {
		return nil, err
	}
	return topicFields(t), nil
}

func publishing(r *taba.PublishResponse, err error) (result, error) {
	if err != nil {
		return nil, err
	}
	return publishFields(r), nil
}

func opPublishBytes(c *taba.Client, p map[string]string) (result, error) {
	data, err := base64.StdEncoding.DecodeString(p["base64"])
	if err != nil {
		return nil, err
	}
	options := publishOptions(p)
	options.ContentType = p["contentType"]
	if options.ContentType == "" {
		options.ContentType = "application/octet-stream"
	}
	return publishing(c.Publish(ctx, p["topic"], data, options))
}

// opPublishBatch publishes a batch of texts (one item per text); bad=true adds an item with an invalid content type.
func opPublishBatch(c *taba.Client, p map[string]string) (result, error) {
	var batch []taba.BatchMessage
	for _, t := range list(p, "texts") {
		batch = append(batch, taba.BatchMessage{Body: []byte(t), ContentType: "text/plain; charset=utf-8", GroupID: p["groupId"]})
	}
	if f := flag(p, "bad"); f != nil && *f {
		batch = append(batch, taba.BatchMessage{Body: []byte("x"), ContentType: "not a type"})
	}
	results, err := c.PublishBatch(ctx, p["topic"], batch)
	if err != nil {
		return nil, err
	}
	return statuses(results), nil
}

// opSettleBatch consumes and settles what came in one batch: action ack or visibility (seconds). stale=true makes
// the receipt of the first one old before the batch: it is given back and taken again.
func opSettleBatch(c *taba.Client, p map[string]string) (result, error) {
	messages, err := c.Consume(ctx, p["topic"], consumeOptions(p))
	if err != nil {
		return nil, err
	}
	if f := flag(p, "stale"); f != nil && *f && len(messages) > 0 {
		if _, err := c.ChangeVisibility(ctx, p["topic"], messages[0], 0); err != nil {
			return nil, err
		}
		if _, err := c.Consume(ctx, p["topic"], &taba.ConsumeOptions{MaxMessages: taba.Int(1), WaitTime: taba.Int(2), VisibilityTimeout: num(p, "visibility")}); err != nil {
			return nil, err
		}
	}
	var results []taba.BatchResult
	if p["action"] == "visibility" {
		seconds, _ := strconv.Atoi(p["seconds"])
		results, err = c.ChangeVisibilityBatch(ctx, p["topic"], messages, seconds)
	} else {
		results, err = c.AckBatch(ctx, p["topic"], messages)
	}
	if err != nil {
		return nil, err
	}
	return statuses(results), nil
}

func opChangeVisibility(c *taba.Client, p map[string]string) (result, error) {
	seconds, _ := strconv.Atoi(p["seconds"])
	r, err := c.ChangeVisibilityByID(ctx, p["topic"], p["id"], p["receipt"], seconds)
	if err != nil {
		return nil, err
	}
	return result{"id": r.ID, "invisibleUntil": r.InvisibleUntil}, nil
}

// opHandle is consuming with a handler: mode ok (deleted), fail (returns an error), keep (Delete = false) or slow
// (holds the message for `hold` seconds, which only works if the invisibility is renewed).
func opHandle(c *taba.Client, p map[string]string) (result, error) {
	var seen []string
	lost := false
	handled, err := c.ConsumeWith(ctx, p["topic"], consumeOptions(p), func(mc *taba.MessageContext) error {
		seen = append(seen, render(mc.Message))
		switch p["mode"] {
		case "fail":
			return fmt.Errorf("the handler failed")
		case "keep":
			mc.Delete = false
		case "slow":
			hold, _ := strconv.ParseFloat(p["hold"], 64)
			time.Sleep(time.Duration(hold * float64(time.Second)))
		}
		lost = mc.LeaseLost()
		return nil
	})
	failed := false
	if err != nil {
		if _, isAPI := err.(*taba.Error); isAPI {
			return nil, err
		}
		failed = true // the handler's own failure
	}
	return result{
		"handled": strconv.Itoa(handled), "failed": strconv.FormatBool(failed),
		"bodies": strings.Join(seen, ","), "leaseLost": strconv.FormatBool(lost),
	}, nil
}

// opSubscribe is the consume loop: it stops after `count` messages or after `timeout` seconds. failFirst makes the
// first one fail.
func opSubscribe(c *taba.Client, p map[string]string) (result, error) {
	cctx, cancel := context.WithCancel(ctx)
	defer cancel()
	seconds, _ := strconv.ParseFloat(p["timeout"], 64)
	timer := time.AfterFunc(time.Duration(seconds*float64(time.Second)), cancel)
	defer timer.Stop()
	count, _ := strconv.Atoi(p["count"])
	var mu sync.Mutex
	var seen []string
	err := c.Subscribe(cctx, p["topic"], &taba.ConsumeOptions{VisibilityTimeout: num(p, "visibility")}, func(m *taba.Message) error {
		mu.Lock()
		defer mu.Unlock()
		seen = append(seen, render(m))
		if len(seen) >= count {
			cancel()
		}
		if p["failFirst"] == "true" && len(seen) == 1 {
			return fmt.Errorf("the handler failed")
		}
		return nil
	}, nil)
	if err != nil {
		return nil, err
	}
	return result{"count": strconv.Itoa(len(seen)), "bodies": strings.Join(seen, ",")}, nil
}

// opCancelConsume: a long polling cancelled from outside must end at once, not at the end of the wait.
func opCancelConsume(c *taba.Client, p map[string]string) (result, error) {
	cctx, cancel := context.WithCancel(ctx)
	defer cancel()
	after, _ := strconv.ParseFloat(p["after"], 64)
	started := time.Now()
	time.AfterFunc(time.Duration(after*float64(time.Second)), cancel)
	_, err := c.Consume(cctx, p["topic"], &taba.ConsumeOptions{WaitTime: taba.Int(20)})
	cancelled := err == context.Canceled
	if err != nil && !cancelled {
		return nil, err
	}
	return result{"cancelled": strconv.FormatBool(cancelled), "fast": strconv.FormatBool(time.Since(started) < 10*time.Second)}, nil
}

// opDrain consumes and acknowledges until `count` messages came or `timeout` seconds passed. The bodies come sorted.
func opDrain(c *taba.Client, p map[string]string) (result, error) {
	count, _ := strconv.Atoi(p["count"])
	seconds, _ := strconv.ParseFloat(p["timeout"], 64)
	deadline := time.Now().Add(time.Duration(seconds * float64(time.Second)))
	var seen []string
	for len(seen) < count && time.Now().Before(deadline) {
		messages, err := c.Consume(ctx, p["topic"], &taba.ConsumeOptions{MaxMessages: taba.Int(10), WaitTime: taba.Int(1), VisibilityTimeout: taba.Int(60)})
		if err != nil {
			return nil, err
		}
		for _, m := range messages {
			seen = append(seen, render(m))
			if err := c.Ack(ctx, p["topic"], m); err != nil {
				return nil, err
			}
		}
	}
	sort.Strings(seen)
	return result{"count": strconv.Itoa(len(seen)), "bodies": strings.Join(seen, ",")}, nil
}

var operations = map[string]operation{
	"createTopic":         opCreateTopic,
	"createOrUpdateTopic": opCreateOrUpdateTopic,
	"getTopic": func(c *taba.Client, p map[string]string) (result, error) {
		t, err := c.GetTopic(ctx, p["name"])
		if err != nil {
			return nil, err
		}
		return topicFields(t), nil
	},
	"listTopics":  opListTopics,
	"updateTopic": opUpdateTopic,
	"deleteTopic": func(c *taba.Client, p map[string]string) (result, error) {
		if b := flag(p, "ifEmpty"); b != nil && *b {
			return result{}, c.DeleteTopicIfEmpty(ctx, p["name"])
		}
		return result{}, c.DeleteTopic(ctx, p["name"])
	},
	"publishText": func(c *taba.Client, p map[string]string) (result, error) {
		return publishing(c.PublishText(ctx, p["topic"], p["body"], publishOptions(p)))
	},
	"publishJson": func(c *taba.Client, p map[string]string) (result, error) {
		return publishing(c.PublishRawJSON(ctx, p["topic"], p["body"], publishOptions(p)))
	},
	"publishBytes": opPublishBytes,
	"publishBatch": opPublishBatch,
	"settleBatch":  opSettleBatch,
	"consume": func(c *taba.Client, p map[string]string) (result, error) {
		messages, err := c.Consume(ctx, p["topic"], consumeOptions(p))
		if err != nil {
			return nil, err
		}
		return messageFields(messages), nil
	},
	"ack": func(c *taba.Client, p map[string]string) (result, error) {
		return result{}, c.AckByID(ctx, p["topic"], p["id"], p["receipt"])
	},
	"changeVisibility": opChangeVisibility,
	"handle":           opHandle,
	"subscribe":        opSubscribe,
	"cancelConsume":    opCancelConsume,
	"drain":            opDrain,
	"sleep": func(c *taba.Client, p map[string]string) (result, error) {
		seconds, _ := strconv.ParseFloat(p["seconds"], 64)
		time.Sleep(time.Duration(seconds * float64(time.Second)))
		return result{}, nil
	},
}

// ---- the run --------------------------------------------------------------------------------------------

func check(r result, expects []expectation) []string {
	var problems []string
	_, failure := r["error"]
	expectsError := false
	for _, e := range expects {
		if e.key == "error" {
			expectsError = true
		}
	}
	if failure && !expectsError {
		problems = append(problems, fmt.Sprintf("unexpected error %s (status %s): %s", r["error"], r["status"], r["message"]))
	}
	if !failure && expectsError {
		problems = append(problems, "expected an error, but the call succeeded")
	}
	for _, e := range expects {
		actual := r[e.key]
		var mismatch bool
		switch {
		case e.op == "~":
			mismatch = !strings.Contains(actual, e.value)
		case e.value == "*":
			mismatch = actual == ""
		default:
			mismatch = actual != e.value
		}
		if mismatch {
			problems = append(problems, fmt.Sprintf("%s %s %q, got %q", e.key, e.op, e.value, actual))
		}
	}
	return problems
}

func main() {
	parseArgs(os.Args[1:])
	clients := map[string]*taba.Client{}
	clientFor := func(p map[string]string) *taba.Client {
		url, token := args["url"], args["token"]
		if v, ok := p["url"]; ok {
			url = v
		}
		if v, ok := p["token"]; ok {
			token = v
		}
		key := url + "|" + token + "|" + p["attempts"]
		if c, ok := clients[key]; ok {
			return c
		}
		config := taba.Config{BaseURL: url, Token: token}
		if n := num(p, "attempts"); n != nil {
			config.MaxAttempts = *n
		}
		c, err := taba.NewClient(config)
		if err != nil {
			fmt.Println("FAIL cannot create the client:", err)
			os.Exit(1)
		}
		clients[key] = c
		return c
	}

	variables := map[string]result{}
	passed, failed, skipped := 0, 0, 0
	for _, path := range scenarios {
		file, err := os.Open(path)
		if err != nil {
			fmt.Println("FAIL", err)
			os.Exit(1)
		}
		scanner := bufio.NewScanner(file)
		for n := 1; scanner.Scan(); n++ {
			text := strings.TrimSpace(scanner.Text())
			if text == "" || strings.HasPrefix(text, "#") {
				continue
			}
			where := fmt.Sprintf("%s:%d", filepath.Base(path), n)
			op, params, expects := parseLine(text, variables)
			run, ok := operations[op]
			if !ok {
				skipped++
				fmt.Printf("SKIP %s %s (not supported by this connector)\n", where, op)
				continue
			}
			r, err := run(clientFor(params), params)
			if err != nil {
				if e, isAPI := err.(*taba.Error); isAPI {
					r = result{"error": e.Code, "status": strconv.Itoa(e.Status), "message": e.Error()}
				} else {
					r = result{"error": "unexpected", "status": "0", "message": err.Error()}
				}
			}
			if as, ok := params["as"]; ok {
				variables[as] = r
			}
			if problems := check(r, expects); len(problems) == 0 {
				passed++
				fmt.Printf("PASS %s %s\n", where, op)
			} else {
				failed++
				fmt.Printf("FAIL %s %s\n", where, text)
				for _, problem := range problems {
					fmt.Println("       " + problem)
				}
			}
		}
		file.Close()
	}
	for _, c := range clients {
		c.Close()
	}
	fmt.Printf("%s: %d passed, %d failed, %d skipped\n", args["lang"], passed, failed, skipped)
	if failed > 0 {
		os.Exit(1)
	}
}
