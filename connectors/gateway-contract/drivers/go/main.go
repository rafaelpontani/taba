// The Go driver of the gateway contract tests (connectors/gateway-contract/README.md): reads the scenario files and
// runs each line through aws-sdk-go-v2, the way an application would. Prints one RESULT line per scenario line.
package main

import (
	"bufio"
	"context"
	"crypto/md5"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"runtime/debug"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/aws/aws-sdk-go-v2/service/sns"
	snstypes "github.com/aws/aws-sdk-go-v2/service/sns/types"
	"github.com/aws/aws-sdk-go-v2/service/sqs"
	sqstypes "github.com/aws/aws-sdk-go-v2/service/sqs/types"
	"github.com/aws/smithy-go"
)

var args = map[string]string{}
var scenarios []string
var creds = map[string][2]string{}

type result struct {
	m    map[string]string
	msgs []string // receipt handles
}

var sqsClient *sqs.Client
var snsClient *sns.Client
var ctx = context.Background()

func makeClients(c *[2]string) {
	opts := []func(*config.LoadOptions) error{config.WithRegion(valueOr("region", "us-east-1")), config.WithRetryMaxAttempts(1),
		config.WithHTTPClient(&http.Client{Transport: plainBodyTransport{http.DefaultTransport.(*http.Transport).Clone()}})}
	if c != nil {
		opts = append(opts, config.WithCredentialsProvider(credentials.NewStaticCredentialsProvider(c[0], c[1], "")))
	}
	cfg, err := config.LoadDefaultConfig(ctx, opts...)
	if err != nil {
		panic(err)
	}
	code := args["endpoint-mode"] == "code"
	sqsClient = sqs.NewFromConfig(cfg, func(o *sqs.Options) {
		if code {
			setEndpointSQS(o, args["sqs-endpoint"])
		}
	})
	snsClient = sns.NewFromConfig(cfg, func(o *sns.Options) {
		if code {
			setEndpointSNS(o, args["sns-endpoint"])
		}
	})
}

func valueOr(k, d string) string {
	if v, ok := args[k]; ok {
		return v
	}
	return d
}

func reachable(raw string) bool {
	u, err := url.Parse(raw)
	if err != nil {
		return false
	}
	port := u.Port()
	if port == "" {
		port = "80"
	}
	c, err := net.DialTimeout("tcp", net.JoinHostPort(u.Hostname(), port), 3*time.Second)
	if err != nil {
		return false
	}
	c.Close()
	return true
}

// ---- the scenario language -------------------------------------------------------------------------------------

var variable = regexp.MustCompile(`\{p\}|\{lang\}|\{prev\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)`)
var expectRe = regexp.MustCompile(`(?s)^([A-Za-z0-9_.]+)(>=|<=|!=|=|~)(.*)$`)

func unquote(s string) string {
	out, err := url.PathUnescape(s)
	if err != nil {
		return s
	}
	return out
}

func expand(value string, vars map[string]*result) string {
	var out strings.Builder
	last := 0
	for _, loc := range variable.FindAllStringSubmatchIndex(value, -1) {
		out.WriteString(unquote(value[last:loc[0]]))
		token := value[loc[0]:loc[1]]
		switch token {
		case "{p}":
			out.WriteString(args["prefix"])
		case "{lang}":
			out.WriteString(args["lang"])
		case "{prev}":
			out.WriteString(args["prev"])
		default:
			if r := vars[value[loc[2]:loc[3]]]; r != nil {
				out.WriteString(r.m[value[loc[4]:loc[5]]])
			}
		}
		last = loc[1]
	}
	out.WriteString(unquote(value[last:]))
	return out.String()
}

type expectation struct{ key, op, want string }

func parseLine(text string, vars map[string]*result) (string, map[string]string, []expectation) {
	head, tail := text, ""
	if i := strings.Index(text, " => "); i >= 0 {
		head, tail = text[:i], text[i+4:]
	}
	parts := strings.Fields(head)
	params := map[string]string{}
	for _, part := range parts[1:] {
		k, v, _ := strings.Cut(part, "=")
		params[k] = expand(v, vars)
	}
	var expects []expectation
	for _, token := range strings.Fields(tail) {
		m := expectRe.FindStringSubmatch(token)
		expects = append(expects, expectation{m[1], m[2], expand(m[3], vars)})
	}
	return parts[0], params, expects
}

func check(r *result, expects []expectation) string {
	for _, e := range expects {
		got := r.m[e.key]
		ok := false
		switch e.op {
		case "=":
			if e.want == "*" {
				ok = got != ""
			} else {
				ok = got == e.want
			}
		case "!=":
			ok = got != e.want
		case "~":
			ok = strings.Contains(got, e.want)
		case ">=", "<=":
			a, err1 := strconv.ParseFloat(got, 64)
			b, err2 := strconv.ParseFloat(e.want, 64)
			ok = err1 == nil && err2 == nil && ((e.op == ">=" && a >= b) || (e.op == "<=" && a <= b))
		}
		if !ok {
			if len(got) > 200 {
				got = got[:200]
			}
			return fmt.Sprintf("%s%s%s but was '%s'", e.key, e.op, e.want, got)
		}
	}
	return ""
}

// ---- the operations --------------------------------------------------------------------------------------------

func md5hex(s string) string { h := md5.Sum([]byte(s)); return hex.EncodeToString(h[:]) }

func pairs(t string) map[string]string {
	out := map[string]string{}
	if t == "" {
		return out
	}
	for _, item := range strings.Split(t, ",") {
		k, v, _ := strings.Cut(item, ":")
		out[k] = v
	}
	return out
}

func sqsAttrsIn(text string) map[string]sqstypes.MessageAttributeValue {
	out := map[string]sqstypes.MessageAttributeValue{}
	if text == "" {
		return out
	}
	for _, item := range strings.Split(text, ",") {
		parts := strings.SplitN(item, ":", 3)
		v := sqstypes.MessageAttributeValue{DataType: aws.String(parts[1])}
		if parts[1] == "Binary" {
			b, _ := base64.StdEncoding.DecodeString(strings.TrimPrefix(parts[2], "b64:"))
			v.BinaryValue = b
		} else {
			v.StringValue = aws.String(parts[2])
		}
		out[parts[0]] = v
	}
	return out
}

func snsAttrsIn(text string) map[string]snstypes.MessageAttributeValue {
	out := map[string]snstypes.MessageAttributeValue{}
	if text == "" {
		return out
	}
	for _, item := range strings.Split(text, ",") {
		parts := strings.SplitN(item, ":", 3)
		v := snstypes.MessageAttributeValue{DataType: aws.String(parts[1])}
		if parts[1] == "Binary" {
			b, _ := base64.StdEncoding.DecodeString(strings.TrimPrefix(parts[2], "b64:"))
			v.BinaryValue = b
		} else {
			v.StringValue = aws.String(parts[2])
		}
		out[parts[0]] = v
	}
	return out
}

func msgAttrsOut(m sqstypes.Message, r map[string]string) {
	for name, a := range m.MessageAttributes {
		typ := aws.ToString(a.DataType)
		if strings.HasPrefix(typ, "Binary") {
			r["msgattr_"+name] = "b64:" + base64.StdEncoding.EncodeToString(a.BinaryValue)
		} else {
			r["msgattr_"+name] = aws.ToString(a.StringValue)
		}
		r["msgattrtype_"+name] = typ
	}
}

func namesOf(urls []string) []string {
	out := []string{}
	for _, u := range urls {
		u = strings.TrimRight(u, "/")
		out = append(out, u[strings.LastIndex(u, "/")+1:])
	}
	sort.Strings(out)
	return out
}

func received(msgs []sqstypes.Message, elapsed time.Duration) *result {
	r := &result{m: map[string]string{}}
	r.m["count"] = strconv.Itoa(len(msgs))
	r.m["elapsedMs"] = strconv.FormatInt(elapsed.Milliseconds(), 10)
	var bodies []string
	for _, m := range msgs {
		bodies = append(bodies, aws.ToString(m.Body))
		r.msgs = append(r.msgs, aws.ToString(m.ReceiptHandle))
	}
	r.m["bodies"] = strings.Join(bodies, ",")
	sort.Strings(bodies)
	r.m["sorted"] = strings.Join(bodies, ",")
	if len(msgs) > 0 {
		m := msgs[0]
		body := aws.ToString(m.Body)
		r.m["body"], r.m["id"], r.m["handle"], r.m["md5"] = body, aws.ToString(m.MessageId), aws.ToString(m.ReceiptHandle), aws.ToString(m.MD5OfBody)
		r.m["md5Ok"] = strconv.FormatBool(md5hex(body) == aws.ToString(m.MD5OfBody))
		r.m["size"] = strconv.Itoa(len([]rune(body)))
		for k, v := range m.Attributes {
			r.m["attr_"+k] = v
		}
		msgAttrsOut(m, r.m)
		var doc map[string]interface{}
		if json.Unmarshal([]byte(body), &doc) == nil {
			for k, v := range doc {
				if s, ok := v.(string); ok {
					r.m["json_"+k] = s
				}
			}
		}
	}
	return r
}

func res(kv ...string) *result {
	r := &result{m: map[string]string{}}
	for i := 0; i+1 < len(kv); i += 2 {
		r.m[kv[i]] = kv[i+1]
	}
	return r
}

func atoi(s string) int { n, _ := strconv.Atoi(s); return n }

func sent(out *sqs.SendMessageOutput, body string) *result {
	return res("id", aws.ToString(out.MessageId), "md5", aws.ToString(out.MD5OfMessageBody),
		"md5Ok", strconv.FormatBool(md5hex(body) == aws.ToString(out.MD5OfMessageBody)), "sequence", aws.ToString(out.SequenceNumber))
}

func attrMap(in map[string]string) *result {
	r := res()
	for k, v := range in {
		r.m["attr_"+k] = v
	}
	return r
}

func handlesOf(p map[string]string, vars map[string]*result) []string {
	if r := vars[strings.TrimPrefix(p["from"], "$")]; r != nil {
		return r.msgs
	}
	return nil
}

func op(name string, p map[string]string, vars map[string]*result) (*result, error) {
	switch name {
	case "createQueue":
		attrs := pairs(p["attrs"])
		if p["fifo"] == "true" {
			attrs["FifoQueue"] = "true"
		}
		if p["redriveArn"] != "" {
			attrs["RedrivePolicy"] = fmt.Sprintf(`{"deadLetterTargetArn":"%s","maxReceiveCount":%s}`, p["redriveArn"], p["redriveMax"])
		}
		in := &sqs.CreateQueueInput{QueueName: aws.String(p["name"])}
		if len(attrs) > 0 {
			in.Attributes = attrs
		}
		out, err := sqsClient.CreateQueue(ctx, in)
		if err != nil {
			return nil, err
		}
		return res("url", aws.ToString(out.QueueUrl), "name", p["name"]), nil
	case "getQueueUrl":
		out, err := sqsClient.GetQueueUrl(ctx, &sqs.GetQueueUrlInput{QueueName: aws.String(p["name"])})
		if err != nil {
			return nil, err
		}
		return res("url", aws.ToString(out.QueueUrl)), nil
	case "deleteQueue":
		_, err := sqsClient.DeleteQueue(ctx, &sqs.DeleteQueueInput{QueueUrl: aws.String(p["queue"])})
		return res(), err
	case "purgeQueue":
		_, err := sqsClient.PurgeQueue(ctx, &sqs.PurgeQueueInput{QueueUrl: aws.String(p["queue"])})
		return res(), err
	case "listQueues":
		out, err := sqsClient.ListQueues(ctx, &sqs.ListQueuesInput{QueueNamePrefix: aws.String(p["prefix"])})
		if err != nil {
			return nil, err
		}
		return res("count", strconv.Itoa(len(out.QueueUrls)), "names", strings.Join(namesOf(out.QueueUrls), ",")), nil
	case "getQueueAttributes":
		var names []sqstypes.QueueAttributeName
		for _, n := range strings.Split(p["names"], ",") {
			names = append(names, sqstypes.QueueAttributeName(n))
		}
		out, err := sqsClient.GetQueueAttributes(ctx, &sqs.GetQueueAttributesInput{QueueUrl: aws.String(p["queue"]), AttributeNames: names})
		if err != nil {
			return nil, err
		}
		return attrMap(out.Attributes), nil
	case "setQueueAttributes":
		_, err := sqsClient.SetQueueAttributes(ctx, &sqs.SetQueueAttributesInput{QueueUrl: aws.String(p["queue"]), Attributes: pairs(p["attrs"])})
		return res(), err
	case "tagQueue":
		_, err := sqsClient.TagQueue(ctx, &sqs.TagQueueInput{QueueUrl: aws.String(p["queue"]), Tags: pairs(p["tags"])})
		return res(), err
	case "untagQueue":
		_, err := sqsClient.UntagQueue(ctx, &sqs.UntagQueueInput{QueueUrl: aws.String(p["queue"]), TagKeys: strings.Split(p["keys"], ",")})
		return res(), err
	case "listQueueTags":
		out, err := sqsClient.ListQueueTags(ctx, &sqs.ListQueueTagsInput{QueueUrl: aws.String(p["queue"])})
		if err != nil {
			return nil, err
		}
		r := res()
		for k, v := range out.Tags {
			r.m["tag_"+k] = v
		}
		return r, nil
	case "send", "sendBig":
		body := p["body"]
		in := &sqs.SendMessageInput{QueueUrl: aws.String(p["queue"])}
		if name == "sendBig" {
			body = strings.Repeat("x", atoi(p["size"]))
		} else {
			if p["delay"] != "" {
				in.DelaySeconds = int32(atoi(p["delay"]))
			}
			if p["msgattrs"] != "" {
				in.MessageAttributes = sqsAttrsIn(p["msgattrs"])
			}
			if p["group"] != "" {
				in.MessageGroupId = aws.String(p["group"])
			}
			if p["dedup"] != "" {
				in.MessageDeduplicationId = aws.String(p["dedup"])
			}
		}
		in.MessageBody = aws.String(body)
		out, err := sqsClient.SendMessage(ctx, in)
		if err != nil {
			return nil, err
		}
		return sent(out, body), nil
	case "sendBatch", "sendBatchTooMany":
		var bodies []string
		if name == "sendBatch" {
			bodies = strings.Split(p["bodies"], ",")
		} else {
			for i := 0; i < atoi(p["count"]); i++ {
				bodies = append(bodies, fmt.Sprintf("m%d", i))
			}
		}
		var delays []string
		if p["delays"] != "" {
			delays = strings.Split(p["delays"], ",")
		}
		var entries []sqstypes.SendMessageBatchRequestEntry
		for i, b := range bodies {
			e := sqstypes.SendMessageBatchRequestEntry{Id: aws.String(strconv.Itoa(i)), MessageBody: aws.String(b)}
			if len(delays) > 0 {
				e.DelaySeconds = int32(atoi(delays[i]))
			}
			if p["group"] != "" {
				e.MessageGroupId = aws.String(p["group"])
			}
			if p["dedupPrefix"] != "" {
				e.MessageDeduplicationId = aws.String(p["dedupPrefix"] + strconv.Itoa(i))
			}
			entries = append(entries, e)
		}
		out, err := sqsClient.SendMessageBatch(ctx, &sqs.SendMessageBatchInput{QueueUrl: aws.String(p["queue"]), Entries: entries})
		if err != nil {
			return nil, err
		}
		return res("ok", strconv.Itoa(len(out.Successful)), "failed", strconv.Itoa(len(out.Failed))), nil
	case "receive":
		in := &sqs.ReceiveMessageInput{QueueUrl: aws.String(p["queue"]), MaxNumberOfMessages: 1}
		if p["max"] != "" {
			in.MaxNumberOfMessages = int32(atoi(p["max"]))
		}
		in.WaitTimeSeconds = int32(atoi(p["wait"]))
		if p["visibility"] != "" {
			in.VisibilityTimeout = int32(atoi(p["visibility"]))
		}
		if p["attrs"] != "" {
			setSystemAttributes(in, strings.Split(p["attrs"], ","))
		}
		if p["msgattrs"] != "" {
			in.MessageAttributeNames = strings.Split(p["msgattrs"], ",")
		}
		t0 := time.Now()
		out, err := sqsClient.ReceiveMessage(ctx, in)
		if err != nil {
			return nil, err
		}
		return received(out.Messages, time.Since(t0)), nil
	case "drainAll":
		want, deadline := atoi(p["count"]), time.Now().Add(time.Duration(atoi(p["timeout"]))*time.Second)
		r := res()
		var bodies []string
		for len(bodies) < want && time.Now().Before(deadline) {
			in := &sqs.ReceiveMessageInput{QueueUrl: aws.String(p["queue"]), MaxNumberOfMessages: 10, WaitTimeSeconds: 1}
			if p["msgattrs"] != "" {
				in.MessageAttributeNames = strings.Split(p["msgattrs"], ",")
			}
			out, err := sqsClient.ReceiveMessage(ctx, in)
			if err != nil {
				return nil, err
			}
			for _, m := range out.Messages {
				bodies = append(bodies, aws.ToString(m.Body))
				msgAttrsOut(m, r.m)
				if _, err := sqsClient.DeleteMessage(ctx, &sqs.DeleteMessageInput{QueueUrl: aws.String(p["queue"]), ReceiptHandle: m.ReceiptHandle}); err != nil {
					return nil, err
				}
			}
		}
		r.m["count"], r.m["bodies"] = strconv.Itoa(len(bodies)), strings.Join(bodies, ",")
		return r, nil
	case "deleteMessage":
		_, err := sqsClient.DeleteMessage(ctx, &sqs.DeleteMessageInput{QueueUrl: aws.String(p["queue"]), ReceiptHandle: aws.String(p["handle"])})
		return res(), err
	case "changeVisibility":
		_, err := sqsClient.ChangeMessageVisibility(ctx, &sqs.ChangeMessageVisibilityInput{QueueUrl: aws.String(p["queue"]), ReceiptHandle: aws.String(p["handle"]), VisibilityTimeout: int32(atoi(p["seconds"]))})
		return res(), err
	case "deleteAll":
		var entries []sqstypes.DeleteMessageBatchRequestEntry
		for i, h := range handlesOf(p, vars) {
			entries = append(entries, sqstypes.DeleteMessageBatchRequestEntry{Id: aws.String(strconv.Itoa(i)), ReceiptHandle: aws.String(h)})
		}
		out, err := sqsClient.DeleteMessageBatch(ctx, &sqs.DeleteMessageBatchInput{QueueUrl: aws.String(p["queue"]), Entries: entries})
		if err != nil {
			return nil, err
		}
		return res("ok", strconv.Itoa(len(out.Successful)), "failed", strconv.Itoa(len(out.Failed))), nil
	case "changeVisibilityAll":
		var entries []sqstypes.ChangeMessageVisibilityBatchRequestEntry
		for i, h := range handlesOf(p, vars) {
			entries = append(entries, sqstypes.ChangeMessageVisibilityBatchRequestEntry{Id: aws.String(strconv.Itoa(i)), ReceiptHandle: aws.String(h), VisibilityTimeout: int32(atoi(p["seconds"]))})
		}
		out, err := sqsClient.ChangeMessageVisibilityBatch(ctx, &sqs.ChangeMessageVisibilityBatchInput{QueueUrl: aws.String(p["queue"]), Entries: entries})
		if err != nil {
			return nil, err
		}
		return res("ok", strconv.Itoa(len(out.Successful)), "failed", strconv.Itoa(len(out.Failed))), nil
	case "allowSnsToSend":
		policy := fmt.Sprintf(`{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"sns.amazonaws.com"},"Action":"sqs:SendMessage","Resource":"%s","Condition":{"ArnEquals":{"aws:SourceArn":"%s"}}}]}`, p["queueArn"], p["topic"])
		_, err := sqsClient.SetQueueAttributes(ctx, &sqs.SetQueueAttributesInput{QueueUrl: aws.String(p["queue"]), Attributes: map[string]string{"Policy": policy}})
		return res(), err
	case "sleep":
		f, _ := strconv.ParseFloat(p["seconds"], 64)
		time.Sleep(time.Duration(f * float64(time.Second)))
		return res(), nil
	case "useCredentials":
		c := creds[p["name"]]
		makeClients(&c)
		return res(), nil
	case "createTopic":
		in := &sns.CreateTopicInput{Name: aws.String(p["name"])}
		if p["fifo"] == "true" {
			in.Attributes = map[string]string{"FifoTopic": "true"}
		}
		out, err := snsClient.CreateTopic(ctx, in)
		if err != nil {
			return nil, err
		}
		return res("arn", aws.ToString(out.TopicArn)), nil
	case "deleteTopic":
		_, err := snsClient.DeleteTopic(ctx, &sns.DeleteTopicInput{TopicArn: aws.String(p["topic"])})
		return res(), err
	case "listTopics":
		var names []string
		var token *string
		for {
			out, err := snsClient.ListTopics(ctx, &sns.ListTopicsInput{NextToken: token})
			if err != nil {
				return nil, err
			}
			for _, t := range out.Topics {
				arn := aws.ToString(t.TopicArn)
				if n := arn[strings.LastIndex(arn, ":")+1:]; strings.HasPrefix(n, p["prefix"]) {
					names = append(names, n)
				}
			}
			if token = out.NextToken; token == nil {
				break
			}
		}
		sort.Strings(names)
		return res("count", strconv.Itoa(len(names)), "names", strings.Join(names, ",")), nil
	case "getTopicAttributes":
		out, err := snsClient.GetTopicAttributes(ctx, &sns.GetTopicAttributesInput{TopicArn: aws.String(p["topic"])})
		if err != nil {
			return nil, err
		}
		return attrMap(out.Attributes), nil
	case "subscribe":
		in := &sns.SubscribeInput{TopicArn: aws.String(p["topic"]), Protocol: aws.String(p["protocol"]), Endpoint: aws.String(p["endpoint"]), ReturnSubscriptionArn: true}
		if p["attrs"] != "" {
			in.Attributes = pairs(p["attrs"])
		}
		out, err := snsClient.Subscribe(ctx, in)
		if err != nil {
			return nil, err
		}
		return res("arn", aws.ToString(out.SubscriptionArn)), nil
	case "unsubscribe":
		_, err := snsClient.Unsubscribe(ctx, &sns.UnsubscribeInput{SubscriptionArn: aws.String(p["sub"])})
		return res(), err
	case "listSubscriptions":
		out, err := snsClient.ListSubscriptionsByTopic(ctx, &sns.ListSubscriptionsByTopicInput{TopicArn: aws.String(p["topic"])})
		if err != nil {
			return nil, err
		}
		var endpoints []string
		protocols := map[string]bool{}
		for _, s := range out.Subscriptions {
			endpoints = append(endpoints, aws.ToString(s.Endpoint))
			protocols[aws.ToString(s.Protocol)] = true
		}
		var plist []string
		for k := range protocols {
			plist = append(plist, k)
		}
		sort.Strings(endpoints)
		sort.Strings(plist)
		return res("count", strconv.Itoa(len(out.Subscriptions)), "endpoints", strings.Join(endpoints, ","), "protocols", strings.Join(plist, ",")), nil
	case "getSubscriptionAttributes":
		out, err := snsClient.GetSubscriptionAttributes(ctx, &sns.GetSubscriptionAttributesInput{SubscriptionArn: aws.String(p["sub"])})
		if err != nil {
			return nil, err
		}
		return attrMap(out.Attributes), nil
	case "setSubscriptionAttributes":
		for k, v := range pairs(p["attrs"]) {
			if _, err := snsClient.SetSubscriptionAttributes(ctx, &sns.SetSubscriptionAttributesInput{SubscriptionArn: aws.String(p["sub"]), AttributeName: aws.String(k), AttributeValue: aws.String(v)}); err != nil {
				return nil, err
			}
		}
		return res(), nil
	case "publish":
		in := &sns.PublishInput{TopicArn: aws.String(p["topic"]), Message: aws.String(p["message"])}
		if p["subject"] != "" {
			in.Subject = aws.String(p["subject"])
		}
		if p["msgattrs"] != "" {
			in.MessageAttributes = snsAttrsIn(p["msgattrs"])
		}
		if p["group"] != "" {
			in.MessageGroupId = aws.String(p["group"])
		}
		if p["dedup"] != "" {
			in.MessageDeduplicationId = aws.String(p["dedup"])
		}
		out, err := snsClient.Publish(ctx, in)
		if err != nil {
			return nil, err
		}
		return res("messageId", aws.ToString(out.MessageId)), nil
	case "publishBatch":
		var entries []snstypes.PublishBatchRequestEntry
		for i, m := range strings.Split(p["messages"], ",") {
			entries = append(entries, snstypes.PublishBatchRequestEntry{Id: aws.String(strconv.Itoa(i)), Message: aws.String(m)})
		}
		out, err := snsClient.PublishBatch(ctx, &sns.PublishBatchInput{TopicArn: aws.String(p["topic"]), PublishBatchRequestEntries: entries})
		if err != nil {
			return nil, err
		}
		return res("ok", strconv.Itoa(len(out.Successful)), "failed", strconv.Itoa(len(out.Failed))), nil
	}
	return nil, fmt.Errorf("operation %s is not implemented", name)
}

func runFile(path string, vars map[string]*result, unreachable bool, counts map[string]int) {
	name := filepath.Base(path)
	f, err := os.Open(path)
	if err != nil {
		panic(err)
	}
	defer f.Close()
	scanner := bufio.NewScanner(f)
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	for n := 1; scanner.Scan(); n++ {
		text := strings.TrimSpace(scanner.Text())
		if text == "" || strings.HasPrefix(text, "#") {
			continue
		}
		status, detail, opName := "PASS", "", strings.Fields(text)[0]
		name2, params, expects := parseLine(text, vars)
		opName = name2
		var expected []string
		for _, e := range expects {
			if e.key == "error" {
				expected = strings.Split(e.want, "|")
			}
		}
		r, err := op(opName, params, vars)
		if err != nil {
			var api smithy.APIError
			if errors.As(err, &api) {
				code := api.ErrorCode()
				code = code[strings.LastIndex(code, "#")+1:] // "com.amazonaws.sqs#QueueDoesNotExist" is the same code
				ok := false
				for _, c := range expected {
					if c == code {
						ok = true
					}
				}
				if !ok {
					status, detail = "FAIL", fmt.Sprintf("error %s: %.160s", code, api.ErrorMessage())
				}
			} else if unreachable {
				status, detail = "CONNECT", "unreachable"
			} else {
				status, detail = "FAIL", fmt.Sprintf("%.200s", err.Error())
			}
		} else {
			if expected != nil {
				status, detail = "FAIL", "expected error "+strings.Join(expected, "|")+" but the call worked"
			} else if problem := check(r, expects); problem != "" {
				status, detail = "FAIL", problem
			}
			if as := params["as"]; as != "" {
				vars[as] = r
			}
		}
		counts[status]++
		if detail != "" {
			detail = " :: " + detail
		}
		fmt.Printf("RESULT %s %s:%d %s%s\n", status, name, n, opName, detail)
	}
}

func main() {
	argv := os.Args[1:]
	for i := 0; i+1 < len(argv); i += 2 {
		key, value := argv[i][2:], argv[i+1]
		switch key {
		case "scenario":
			scenarios = append(scenarios, value)
		case "cred":
			name, pair, _ := strings.Cut(value, "=")
			ak, sk, _ := strings.Cut(pair, ":")
			creds[name] = [2]string{ak, sk}
		default:
			args[key] = value
		}
	}
	makeClients(nil)
	unreachable := !reachable(args["sqs-endpoint"]) || !reachable(args["sns-endpoint"])
	vars := map[string]*result{}
	counts := map[string]int{}
	for _, s := range scenarios {
		runFile(s, vars, unreachable, counts)
	}
	sdk := ""
	if info, ok := debug.ReadBuildInfo(); ok {
		for _, d := range info.Deps {
			if strings.HasSuffix(d.Path, "/service/sqs") || strings.HasSuffix(d.Path, "/service/sns") || d.Path == "github.com/aws/aws-sdk-go-v2" {
				sdk += d.Path[strings.LastIndex(d.Path, "/")+1:] + "-" + d.Version + " "
			}
		}
	}
	args["sdk"] = strings.TrimSpace(sdk)
	fmt.Printf("SUMMARY lang=%s variant=%s sdk=%s pass=%d fail=%d connect=%d\n", args["lang"], args["variant"], args["sdk"], counts["PASS"], counts["FAIL"], counts["CONNECT"])
}

// plainBodyTransport hands the request body to net/http without its WriteTo. The SDK (smithy-go, every version up to
// v1.28 at least) closes the request body as soon as the response headers arrive, and its safeWriteToReadCloser answers
// WriteTo after the close with io.EOF instead of (0, nil). net/http, after writing the body, reads it once more to check
// that nothing is left (transfer.go, writeBody); when the machine is loaded that read comes after the close, the EOF is
// taken as a write error and the writeLoop closes the connection in the middle of the response: ReceiveMessage of a large
// message failed with "use of closed network connection" (doc 36f). A plain Read after the close is (0, io.EOF), which
// io.Copy takes as the end.
type plainBodyTransport struct{ next http.RoundTripper }

type plainBody struct{ io.ReadCloser }

func (t plainBodyTransport) RoundTrip(r *http.Request) (*http.Response, error) {
	if r.Body != nil && r.Body != http.NoBody {
		r = r.Clone(r.Context())
		r.Body = plainBody{r.Body}
	}
	return t.next.RoundTrip(r)
}
