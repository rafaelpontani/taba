import java.net.URI;
import java.time.Duration;
import java.util.*;
import java.util.stream.Collectors;
import software.amazon.awssdk.auth.credentials.AwsBasicCredentials;
import software.amazon.awssdk.auth.credentials.StaticCredentialsProvider;
import software.amazon.awssdk.core.SdkBytes;
import software.amazon.awssdk.core.exception.SdkClientException;
import software.amazon.awssdk.core.retry.RetryPolicy;
import software.amazon.awssdk.awscore.exception.AwsServiceException;
import software.amazon.awssdk.regions.Region;
import software.amazon.awssdk.services.sns.SnsClient;
import software.amazon.awssdk.services.sns.SnsClientBuilder;
import software.amazon.awssdk.services.sns.model.*;
import software.amazon.awssdk.services.sqs.SqsClient;
import software.amazon.awssdk.services.sqs.SqsClientBuilder;
import software.amazon.awssdk.services.sqs.model.*;
import software.amazon.awssdk.services.sqs.model.MessageAttributeValue;

/** The AWS SDK for Java v2 (software.amazon.awssdk). */
class OpsV2 implements Ops {
    SqsClient sqs;
    SnsClient sns;

    OpsV2() {
        make(null);
    }

    void make(StaticCredentialsProvider credentials) {
        Region region = Region.of(Main.arg("region", "us-east-1"));
        boolean code = Main.arg("endpoint-mode", "env").equals("code");
        SqsClientBuilder a = SqsClient.builder().region(region)
            .overrideConfiguration(c -> c.retryPolicy(RetryPolicy.none()).apiCallTimeout(Duration.ofSeconds(60)));
        SnsClientBuilder b = SnsClient.builder().region(region)
            .overrideConfiguration(c -> c.retryPolicy(RetryPolicy.none()).apiCallTimeout(Duration.ofSeconds(60)));
        if (credentials != null) {
            a.credentialsProvider(credentials);
            b.credentialsProvider(credentials);
        }
        if (code) {
            a.endpointOverride(URI.create(Main.arg("sqs-endpoint", "")));
            b.endpointOverride(URI.create(Main.arg("sns-endpoint", "")));
        }
        sqs = a.build();
        sns = b.build();
    }

    public String errorCode(Throwable t) {
        return t instanceof AwsServiceException ? ((AwsServiceException) t).awsErrorDetails().errorCode() : null;
    }

    public String sdkName() {
        return "aws-sdk-java-v2-" + Main.arg("sdk", "");
    }

    static Main.Result batch(int ok, int failed) {
        return new Main.Result().with("ok", String.valueOf(ok), "failed", String.valueOf(failed));
    }

    Map<String, MessageAttributeValue> sqsAttrs(String text) {
        Map<String, MessageAttributeValue> out = new LinkedHashMap<>();
        for (String[] i : Main.attrItems(text)) {
            MessageAttributeValue.Builder v = MessageAttributeValue.builder().dataType(i[1]);
            out.put(i[0], i[1].equals("Binary") ? v.binaryValue(SdkBytes.fromByteArray(Main.binary(i[2]))).build() : v.stringValue(i[2]).build());
        }
        return out;
    }

    Map<String, software.amazon.awssdk.services.sns.model.MessageAttributeValue> snsAttrs(String text) {
        Map<String, software.amazon.awssdk.services.sns.model.MessageAttributeValue> out = new LinkedHashMap<>();
        for (String[] i : Main.attrItems(text)) {
            software.amazon.awssdk.services.sns.model.MessageAttributeValue.Builder v =
                software.amazon.awssdk.services.sns.model.MessageAttributeValue.builder().dataType(i[1]);
            out.put(i[0], i[1].equals("Binary") ? v.binaryValue(SdkBytes.fromByteArray(Main.binary(i[2]))).build() : v.stringValue(i[2]).build());
        }
        return out;
    }

    Main.Msg msg(Message m) {
        Main.Msg r = new Main.Msg();
        r.id = m.messageId();
        r.body = m.body();
        r.handle = m.receiptHandle();
        r.md5 = m.md5OfBody();
        for (Map.Entry<String, String> e : m.attributesAsStrings().entrySet()) r.attributes.put(e.getKey(), e.getValue());
        for (Map.Entry<String, MessageAttributeValue> e : m.messageAttributes().entrySet()) {
            MessageAttributeValue a = e.getValue();
            String value = a.dataType().startsWith("Binary") ? "b64:" + Base64.getEncoder().encodeToString(a.binaryValue().asByteArray()) : a.stringValue();
            r.msgAttributes.put(e.getKey(), new String[] {a.dataType(), value});
        }
        return r;
    }

    Main.Result attrs(Map<String, String> in) {
        Main.Result r = new Main.Result();
        in.forEach((k, v) -> r.m.put("attr_" + k, v));
        return r;
    }

    Main.Result sent(SendMessageResponse o, String body) {
        return new Main.Result().with("id", o.messageId(), "md5", o.md5OfMessageBody(), "md5Ok", String.valueOf(Main.md5(body).equals(o.md5OfMessageBody())),
            "sequence", o.sequenceNumber() == null ? "" : o.sequenceNumber());
    }

    List<String> handlesOf(Map<String, String> p, Map<String, Main.Result> vars) {
        Main.Result r = vars.get(p.get("from").replaceFirst("^\\$", ""));
        return r == null ? new ArrayList<>() : r.handles;
    }

    public Main.Result run(String op, Map<String, String> p, Map<String, Main.Result> vars) throws Exception {
        switch (op) {
            case "createQueue": {
                Map<String, String> a = Main.pairs(p.get("attrs"));
                if ("true".equals(p.get("fifo"))) a.put("FifoQueue", "true");
                if (p.containsKey("redriveArn")) a.put("RedrivePolicy", Main.redrive(p));
                CreateQueueRequest.Builder b = CreateQueueRequest.builder().queueName(p.get("name"));
                if (!a.isEmpty()) b.attributesWithStrings(a);
                return new Main.Result().with("url", sqs.createQueue(b.build()).queueUrl(), "name", p.get("name"));
            }
            case "getQueueUrl":
                return new Main.Result().with("url", sqs.getQueueUrl(GetQueueUrlRequest.builder().queueName(p.get("name")).build()).queueUrl());
            case "deleteQueue":
                sqs.deleteQueue(DeleteQueueRequest.builder().queueUrl(p.get("queue")).build());
                return new Main.Result();
            case "purgeQueue":
                sqs.purgeQueue(PurgeQueueRequest.builder().queueUrl(p.get("queue")).build());
                return new Main.Result();
            case "listQueues": {
                List<String> urls = sqs.listQueues(ListQueuesRequest.builder().queueNamePrefix(p.get("prefix")).build()).queueUrls();
                List<String> names = urls.stream().map(Main::lastSegment).sorted().collect(Collectors.toList());
                return new Main.Result().with("count", String.valueOf(urls.size()), "names", String.join(",", names));
            }
            case "getQueueAttributes": {
                GetQueueAttributesRequest.Builder b = GetQueueAttributesRequest.builder().queueUrl(p.get("queue")).attributeNamesWithStrings(p.get("names").split(","));
                return attrs(sqs.getQueueAttributes(b.build()).attributesAsStrings());
            }
            case "setQueueAttributes":
                sqs.setQueueAttributes(SetQueueAttributesRequest.builder().queueUrl(p.get("queue")).attributesWithStrings(Main.pairs(p.get("attrs"))).build());
                return new Main.Result();
            case "tagQueue":
                sqs.tagQueue(TagQueueRequest.builder().queueUrl(p.get("queue")).tags(Main.pairs(p.get("tags"))).build());
                return new Main.Result();
            case "untagQueue":
                sqs.untagQueue(UntagQueueRequest.builder().queueUrl(p.get("queue")).tagKeys(p.get("keys").split(",")).build());
                return new Main.Result();
            case "listQueueTags": {
                Main.Result r = new Main.Result();
                sqs.listQueueTags(ListQueueTagsRequest.builder().queueUrl(p.get("queue")).build()).tags().forEach((k, v) -> r.m.put("tag_" + k, v));
                return r;
            }
            case "send":
            case "sendBig": {
                String body = op.equals("sendBig") ? "x".repeat(Integer.parseInt(p.get("size"))) : p.get("body");
                SendMessageRequest.Builder b = SendMessageRequest.builder().queueUrl(p.get("queue")).messageBody(body);
                if (op.equals("send")) {
                    if (p.containsKey("delay")) b.delaySeconds(Integer.parseInt(p.get("delay")));
                    if (p.containsKey("msgattrs")) b.messageAttributes(sqsAttrs(p.get("msgattrs")));
                    if (p.containsKey("group")) b.messageGroupId(p.get("group"));
                    if (p.containsKey("dedup")) b.messageDeduplicationId(p.get("dedup"));
                }
                return sent(sqs.sendMessage(b.build()), body);
            }
            case "sendBatch":
            case "sendBatchTooMany": {
                List<String> bodies = new ArrayList<>();
                if (op.equals("sendBatch")) bodies.addAll(Arrays.asList(p.get("bodies").split(",")));
                else for (int i = 0; i < Integer.parseInt(p.get("count")); i++) bodies.add("m" + i);
                String[] delays = p.containsKey("delays") ? p.get("delays").split(",") : new String[0];
                List<SendMessageBatchRequestEntry> entries = new ArrayList<>();
                for (int i = 0; i < bodies.size(); i++) {
                    SendMessageBatchRequestEntry.Builder e = SendMessageBatchRequestEntry.builder().id(String.valueOf(i)).messageBody(bodies.get(i));
                    if (delays.length > 0) e.delaySeconds(Integer.parseInt(delays[i]));
                    if (p.containsKey("group")) e.messageGroupId(p.get("group"));
                    if (p.containsKey("dedupPrefix")) e.messageDeduplicationId(p.get("dedupPrefix") + i);
                    entries.add(e.build());
                }
                SendMessageBatchResponse o = sqs.sendMessageBatch(SendMessageBatchRequest.builder().queueUrl(p.get("queue")).entries(entries).build());
                return batch(o.successful().size(), o.failed().size());
            }
            case "receive": {
                ReceiveMessageRequest.Builder b = ReceiveMessageRequest.builder().queueUrl(p.get("queue"))
                    .maxNumberOfMessages(Integer.parseInt(p.getOrDefault("max", "1"))).waitTimeSeconds(Integer.parseInt(p.getOrDefault("wait", "0")));
                if (p.containsKey("visibility")) b.visibilityTimeout(Integer.parseInt(p.get("visibility")));
                if (p.containsKey("attrs")) b.attributeNamesWithStrings(p.get("attrs").split(","));
                if (p.containsKey("msgattrs")) b.messageAttributeNames(p.get("msgattrs").split(","));
                long t0 = System.currentTimeMillis();
                List<Main.Msg> msgs = sqs.receiveMessage(b.build()).messages().stream().map(this::msg).collect(Collectors.toList());
                return Main.received(msgs, System.currentTimeMillis() - t0);
            }
            case "drainAll": {
                int want = Integer.parseInt(p.get("count"));
                long deadline = System.currentTimeMillis() + Long.parseLong(p.get("timeout")) * 1000;
                Main.Result r = new Main.Result();
                List<String> bodies = new ArrayList<>();
                while (bodies.size() < want && System.currentTimeMillis() < deadline) {
                    ReceiveMessageRequest.Builder b = ReceiveMessageRequest.builder().queueUrl(p.get("queue")).maxNumberOfMessages(10).waitTimeSeconds(1);
                    if (p.containsKey("msgattrs")) b.messageAttributeNames(p.get("msgattrs").split(","));
                    for (Message m : sqs.receiveMessage(b.build()).messages()) {
                        Main.Msg x = msg(m);
                        bodies.add(x.body);
                        Main.msgAttrs(x, r.m);
                        sqs.deleteMessage(DeleteMessageRequest.builder().queueUrl(p.get("queue")).receiptHandle(x.handle).build());
                    }
                }
                return r.with("count", String.valueOf(bodies.size()), "bodies", String.join(",", bodies));
            }
            case "deleteMessage":
                sqs.deleteMessage(DeleteMessageRequest.builder().queueUrl(p.get("queue")).receiptHandle(p.get("handle")).build());
                return new Main.Result();
            case "changeVisibility":
                sqs.changeMessageVisibility(ChangeMessageVisibilityRequest.builder().queueUrl(p.get("queue")).receiptHandle(p.get("handle"))
                    .visibilityTimeout(Integer.parseInt(p.get("seconds"))).build());
                return new Main.Result();
            case "deleteAll": {
                List<DeleteMessageBatchRequestEntry> entries = new ArrayList<>();
                List<String> hs = handlesOf(p, vars);
                for (int i = 0; i < hs.size(); i++) entries.add(DeleteMessageBatchRequestEntry.builder().id(String.valueOf(i)).receiptHandle(hs.get(i)).build());
                DeleteMessageBatchResponse o = sqs.deleteMessageBatch(DeleteMessageBatchRequest.builder().queueUrl(p.get("queue")).entries(entries).build());
                return batch(o.successful().size(), o.failed().size());
            }
            case "changeVisibilityAll": {
                List<ChangeMessageVisibilityBatchRequestEntry> entries = new ArrayList<>();
                List<String> hs = handlesOf(p, vars);
                for (int i = 0; i < hs.size(); i++)
                    entries.add(ChangeMessageVisibilityBatchRequestEntry.builder().id(String.valueOf(i)).receiptHandle(hs.get(i)).visibilityTimeout(Integer.parseInt(p.get("seconds"))).build());
                ChangeMessageVisibilityBatchResponse o = sqs.changeMessageVisibilityBatch(ChangeMessageVisibilityBatchRequest.builder().queueUrl(p.get("queue")).entries(entries).build());
                return batch(o.successful().size(), o.failed().size());
            }
            case "allowSnsToSend":
                sqs.setQueueAttributes(SetQueueAttributesRequest.builder().queueUrl(p.get("queue")).attributesWithStrings(Map.of("Policy", Main.policy(p))).build());
                return new Main.Result();
            case "sleep":
                Thread.sleep((long) (Double.parseDouble(p.get("seconds")) * 1000));
                return new Main.Result();
            case "useCredentials": {
                String[] c = Main.CREDS.get(p.get("name"));
                make(StaticCredentialsProvider.create(AwsBasicCredentials.create(c[0], c[1])));
                return new Main.Result();
            }
            case "createTopic": {
                CreateTopicRequest.Builder b = CreateTopicRequest.builder().name(p.get("name"));
                if ("true".equals(p.get("fifo"))) b.attributes(Map.of("FifoTopic", "true"));
                return new Main.Result().with("arn", sns.createTopic(b.build()).topicArn());
            }
            case "deleteTopic":
                sns.deleteTopic(DeleteTopicRequest.builder().topicArn(p.get("topic")).build());
                return new Main.Result();
            case "listTopics": {
                List<String> names = new ArrayList<>();
                String token = null;
                do {
                    ListTopicsResponse o = sns.listTopics(ListTopicsRequest.builder().nextToken(token).build());
                    for (Topic t : o.topics()) {
                        String n = t.topicArn().substring(t.topicArn().lastIndexOf(':') + 1);
                        if (n.startsWith(p.get("prefix"))) names.add(n);
                    }
                    token = o.nextToken();
                } while (token != null && !token.isEmpty());
                Collections.sort(names);
                return new Main.Result().with("count", String.valueOf(names.size()), "names", String.join(",", names));
            }
            case "getTopicAttributes":
                return attrs(sns.getTopicAttributes(GetTopicAttributesRequest.builder().topicArn(p.get("topic")).build()).attributes());
            case "subscribe": {
                SubscribeRequest.Builder b = SubscribeRequest.builder().topicArn(p.get("topic")).protocol(p.get("protocol")).endpoint(p.get("endpoint")).returnSubscriptionArn(true);
                if (p.containsKey("attrs")) b.attributes(Main.pairs(p.get("attrs")));
                return new Main.Result().with("arn", sns.subscribe(b.build()).subscriptionArn());
            }
            case "unsubscribe":
                sns.unsubscribe(UnsubscribeRequest.builder().subscriptionArn(p.get("sub")).build());
                return new Main.Result();
            case "listSubscriptions": {
                List<Subscription> subs = sns.listSubscriptionsByTopic(ListSubscriptionsByTopicRequest.builder().topicArn(p.get("topic")).build()).subscriptions();
                return new Main.Result().with("count", String.valueOf(subs.size()),
                    "endpoints", subs.stream().map(Subscription::endpoint).sorted().collect(Collectors.joining(",")),
                    "protocols", subs.stream().map(Subscription::protocol).distinct().sorted().collect(Collectors.joining(",")));
            }
            case "getSubscriptionAttributes":
                return attrs(sns.getSubscriptionAttributes(GetSubscriptionAttributesRequest.builder().subscriptionArn(p.get("sub")).build()).attributes());
            case "setSubscriptionAttributes":
                for (Map.Entry<String, String> e : Main.pairs(p.get("attrs")).entrySet())
                    sns.setSubscriptionAttributes(SetSubscriptionAttributesRequest.builder().subscriptionArn(p.get("sub")).attributeName(e.getKey()).attributeValue(e.getValue()).build());
                return new Main.Result();
            case "publish": {
                PublishRequest.Builder b = PublishRequest.builder().topicArn(p.get("topic")).message(p.get("message"));
                if (p.containsKey("subject")) b.subject(p.get("subject"));
                if (p.containsKey("msgattrs")) b.messageAttributes(snsAttrs(p.get("msgattrs")));
                if (p.containsKey("group")) b.messageGroupId(p.get("group"));
                if (p.containsKey("dedup")) b.messageDeduplicationId(p.get("dedup"));
                return new Main.Result().with("messageId", sns.publish(b.build()).messageId());
            }
            case "publishBatch": {
                String[] messages = p.get("messages").split(",");
                List<PublishBatchRequestEntry> entries = new ArrayList<>();
                for (int i = 0; i < messages.length; i++) entries.add(PublishBatchRequestEntry.builder().id(String.valueOf(i)).message(messages[i]).build());
                PublishBatchResponse o = sns.publishBatch(PublishBatchRequest.builder().topicArn(p.get("topic")).publishBatchRequestEntries(entries).build());
                return batch(o.successful().size(), o.failed().size());
            }
            default:
                throw new IllegalStateException("operation " + op + " is not implemented");
        }
    }
}
