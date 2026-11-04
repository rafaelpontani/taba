import com.amazonaws.AmazonServiceException;
import com.amazonaws.ClientConfiguration;
import com.amazonaws.auth.AWSStaticCredentialsProvider;
import com.amazonaws.auth.BasicAWSCredentials;
import com.amazonaws.client.builder.AwsClientBuilder.EndpointConfiguration;
import com.amazonaws.services.sns.AmazonSNS;
import com.amazonaws.services.sns.AmazonSNSClientBuilder;
import com.amazonaws.services.sns.model.*;
import com.amazonaws.services.sqs.AmazonSQS;
import com.amazonaws.services.sqs.AmazonSQSClientBuilder;
import com.amazonaws.services.sqs.model.*;
import com.amazonaws.services.sqs.model.MessageAttributeValue;
import java.nio.ByteBuffer;
import java.util.*;
import java.util.stream.Collectors;

/** The AWS SDK for Java v1 (com.amazonaws). It does not read AWS_ENDPOINT_URL_*, so the endpoint is always in the code. */
class OpsV1 implements Ops {
    AmazonSQS sqs;
    AmazonSNS sns;

    OpsV1() {
        make(null);
    }

    void make(AWSStaticCredentialsProvider credentials) {
        String region = Main.arg("region", "us-east-1");
        ClientConfiguration cc = new ClientConfiguration().withMaxErrorRetry(0).withRequestTimeout(60000).withSocketTimeout(60000);
        AmazonSQSClientBuilder a = AmazonSQSClientBuilder.standard().withClientConfiguration(cc)
            .withEndpointConfiguration(new EndpointConfiguration(Main.arg("sqs-endpoint", ""), region));
        AmazonSNSClientBuilder b = AmazonSNSClientBuilder.standard().withClientConfiguration(cc)
            .withEndpointConfiguration(new EndpointConfiguration(Main.arg("sns-endpoint", ""), region));
        if (credentials != null) {
            a.withCredentials(credentials);
            b.withCredentials(credentials);
        }
        sqs = a.build();
        sns = b.build();
    }

    public String errorCode(Throwable t) {
        return t instanceof AmazonServiceException ? ((AmazonServiceException) t).getErrorCode() : null;
    }

    public String sdkName() {
        return "aws-sdk-java-v1-" + Main.arg("sdk", "");
    }

    static Main.Result batch(int ok, int failed) {
        return new Main.Result().with("ok", String.valueOf(ok), "failed", String.valueOf(failed));
    }

    Map<String, MessageAttributeValue> sqsAttrs(String text) {
        Map<String, MessageAttributeValue> out = new LinkedHashMap<>();
        for (String[] i : Main.attrItems(text)) {
            MessageAttributeValue v = new MessageAttributeValue().withDataType(i[1]);
            if (i[1].equals("Binary")) v.withBinaryValue(ByteBuffer.wrap(Main.binary(i[2])));
            else v.withStringValue(i[2]);
            out.put(i[0], v);
        }
        return out;
    }

    Map<String, com.amazonaws.services.sns.model.MessageAttributeValue> snsAttrs(String text) {
        Map<String, com.amazonaws.services.sns.model.MessageAttributeValue> out = new LinkedHashMap<>();
        for (String[] i : Main.attrItems(text)) {
            com.amazonaws.services.sns.model.MessageAttributeValue v = new com.amazonaws.services.sns.model.MessageAttributeValue().withDataType(i[1]);
            if (i[1].equals("Binary")) v.withBinaryValue(ByteBuffer.wrap(Main.binary(i[2])));
            else v.withStringValue(i[2]);
            out.put(i[0], v);
        }
        return out;
    }

    Main.Msg msg(Message m) {
        Main.Msg r = new Main.Msg();
        r.id = m.getMessageId();
        r.body = m.getBody();
        r.handle = m.getReceiptHandle();
        r.md5 = m.getMD5OfBody();
        r.attributes.putAll(m.getAttributes());
        for (Map.Entry<String, MessageAttributeValue> e : m.getMessageAttributes().entrySet()) {
            MessageAttributeValue a = e.getValue();
            String value;
            if (a.getDataType().startsWith("Binary")) {
                ByteBuffer bb = a.getBinaryValue().duplicate();
                byte[] bytes = new byte[bb.remaining()];
                bb.get(bytes);
                value = "b64:" + Base64.getEncoder().encodeToString(bytes);
            } else value = a.getStringValue();
            r.msgAttributes.put(e.getKey(), new String[] {a.getDataType(), value});
        }
        return r;
    }

    Main.Result attrs(Map<String, String> in) {
        Main.Result r = new Main.Result();
        in.forEach((k, v) -> r.m.put("attr_" + k, v));
        return r;
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
                CreateQueueRequest r = new CreateQueueRequest(p.get("name"));
                if (!a.isEmpty()) r.setAttributes(a);
                return new Main.Result().with("url", sqs.createQueue(r).getQueueUrl(), "name", p.get("name"));
            }
            case "getQueueUrl":
                return new Main.Result().with("url", sqs.getQueueUrl(p.get("name")).getQueueUrl());
            case "deleteQueue":
                sqs.deleteQueue(p.get("queue"));
                return new Main.Result();
            case "purgeQueue":
                sqs.purgeQueue(new PurgeQueueRequest(p.get("queue")));
                return new Main.Result();
            case "listQueues": {
                List<String> urls = sqs.listQueues(new ListQueuesRequest(p.get("prefix"))).getQueueUrls();
                List<String> names = urls.stream().map(Main::lastSegment).sorted().collect(Collectors.toList());
                return new Main.Result().with("count", String.valueOf(urls.size()), "names", String.join(",", names));
            }
            case "getQueueAttributes":
                return attrs(sqs.getQueueAttributes(new GetQueueAttributesRequest(p.get("queue"), Arrays.asList(p.get("names").split(",")))).getAttributes());
            case "setQueueAttributes":
                sqs.setQueueAttributes(new SetQueueAttributesRequest(p.get("queue"), Main.pairs(p.get("attrs"))));
                return new Main.Result();
            case "tagQueue":
                sqs.tagQueue(new TagQueueRequest(p.get("queue"), Main.pairs(p.get("tags"))));
                return new Main.Result();
            case "untagQueue":
                sqs.untagQueue(new UntagQueueRequest(p.get("queue"), Arrays.asList(p.get("keys").split(","))));
                return new Main.Result();
            case "listQueueTags": {
                Main.Result r = new Main.Result();
                sqs.listQueueTags(new ListQueueTagsRequest(p.get("queue"))).getTags().forEach((k, v) -> r.m.put("tag_" + k, v));
                return r;
            }
            case "send":
            case "sendBig": {
                String body = op.equals("sendBig") ? new String(new char[Integer.parseInt(p.get("size"))]).replace('\0', 'x') : p.get("body");
                SendMessageRequest r = new SendMessageRequest(p.get("queue"), body);
                if (op.equals("send")) {
                    if (p.containsKey("delay")) r.setDelaySeconds(Integer.parseInt(p.get("delay")));
                    if (p.containsKey("msgattrs")) r.setMessageAttributes(sqsAttrs(p.get("msgattrs")));
                    if (p.containsKey("group")) r.setMessageGroupId(p.get("group"));
                    if (p.containsKey("dedup")) r.setMessageDeduplicationId(p.get("dedup"));
                }
                SendMessageResult o = sqs.sendMessage(r);
                return new Main.Result().with("id", o.getMessageId(), "md5", o.getMD5OfMessageBody(), "md5Ok", String.valueOf(Main.md5(body).equals(o.getMD5OfMessageBody())),
                    "sequence", o.getSequenceNumber() == null ? "" : o.getSequenceNumber());
            }
            case "sendBatch":
            case "sendBatchTooMany": {
                List<String> bodies = new ArrayList<>();
                if (op.equals("sendBatch")) bodies.addAll(Arrays.asList(p.get("bodies").split(",")));
                else for (int i = 0; i < Integer.parseInt(p.get("count")); i++) bodies.add("m" + i);
                String[] delays = p.containsKey("delays") ? p.get("delays").split(",") : new String[0];
                List<SendMessageBatchRequestEntry> entries = new ArrayList<>();
                for (int i = 0; i < bodies.size(); i++) {
                    SendMessageBatchRequestEntry e = new SendMessageBatchRequestEntry(String.valueOf(i), bodies.get(i));
                    if (delays.length > 0) e.setDelaySeconds(Integer.parseInt(delays[i]));
                    if (p.containsKey("group")) e.setMessageGroupId(p.get("group"));
                    if (p.containsKey("dedupPrefix")) e.setMessageDeduplicationId(p.get("dedupPrefix") + i);
                    entries.add(e);
                }
                SendMessageBatchResult o = sqs.sendMessageBatch(new SendMessageBatchRequest(p.get("queue"), entries));
                return batch(o.getSuccessful().size(), o.getFailed().size());
            }
            case "receive": {
                ReceiveMessageRequest r = new ReceiveMessageRequest(p.get("queue"))
                    .withMaxNumberOfMessages(Integer.parseInt(p.getOrDefault("max", "1"))).withWaitTimeSeconds(Integer.parseInt(p.getOrDefault("wait", "0")));
                if (p.containsKey("visibility")) r.setVisibilityTimeout(Integer.parseInt(p.get("visibility")));
                if (p.containsKey("attrs")) r.setAttributeNames(Arrays.asList(p.get("attrs").split(",")));
                if (p.containsKey("msgattrs")) r.setMessageAttributeNames(Arrays.asList(p.get("msgattrs").split(",")));
                long t0 = System.currentTimeMillis();
                List<Main.Msg> msgs = sqs.receiveMessage(r).getMessages().stream().map(this::msg).collect(Collectors.toList());
                return Main.received(msgs, System.currentTimeMillis() - t0);
            }
            case "drainAll": {
                int want = Integer.parseInt(p.get("count"));
                long deadline = System.currentTimeMillis() + Long.parseLong(p.get("timeout")) * 1000;
                Main.Result res = new Main.Result();
                List<String> bodies = new ArrayList<>();
                while (bodies.size() < want && System.currentTimeMillis() < deadline) {
                    ReceiveMessageRequest r = new ReceiveMessageRequest(p.get("queue")).withMaxNumberOfMessages(10).withWaitTimeSeconds(1);
                    if (p.containsKey("msgattrs")) r.setMessageAttributeNames(Arrays.asList(p.get("msgattrs").split(",")));
                    for (Message m : sqs.receiveMessage(r).getMessages()) {
                        Main.Msg x = msg(m);
                        bodies.add(x.body);
                        Main.msgAttrs(x, res.m);
                        sqs.deleteMessage(p.get("queue"), x.handle);
                    }
                }
                return res.with("count", String.valueOf(bodies.size()), "bodies", String.join(",", bodies));
            }
            case "deleteMessage":
                sqs.deleteMessage(p.get("queue"), p.get("handle"));
                return new Main.Result();
            case "changeVisibility":
                sqs.changeMessageVisibility(p.get("queue"), p.get("handle"), Integer.parseInt(p.get("seconds")));
                return new Main.Result();
            case "deleteAll": {
                List<DeleteMessageBatchRequestEntry> entries = new ArrayList<>();
                List<String> hs = handlesOf(p, vars);
                for (int i = 0; i < hs.size(); i++) entries.add(new DeleteMessageBatchRequestEntry(String.valueOf(i), hs.get(i)));
                DeleteMessageBatchResult o = sqs.deleteMessageBatch(new DeleteMessageBatchRequest(p.get("queue"), entries));
                return batch(o.getSuccessful().size(), o.getFailed().size());
            }
            case "changeVisibilityAll": {
                List<ChangeMessageVisibilityBatchRequestEntry> entries = new ArrayList<>();
                List<String> hs = handlesOf(p, vars);
                for (int i = 0; i < hs.size(); i++)
                    entries.add(new ChangeMessageVisibilityBatchRequestEntry(String.valueOf(i), hs.get(i)).withVisibilityTimeout(Integer.parseInt(p.get("seconds"))));
                ChangeMessageVisibilityBatchResult o = sqs.changeMessageVisibilityBatch(new ChangeMessageVisibilityBatchRequest(p.get("queue"), entries));
                return batch(o.getSuccessful().size(), o.getFailed().size());
            }
            case "allowSnsToSend":
                sqs.setQueueAttributes(new SetQueueAttributesRequest(p.get("queue"), Collections.singletonMap("Policy", Main.policy(p))));
                return new Main.Result();
            case "sleep":
                Thread.sleep((long) (Double.parseDouble(p.get("seconds")) * 1000));
                return new Main.Result();
            case "useCredentials": {
                String[] c = Main.CREDS.get(p.get("name"));
                make(new AWSStaticCredentialsProvider(new BasicAWSCredentials(c[0], c[1])));
                return new Main.Result();
            }
            case "createTopic": {
                CreateTopicRequest r = new CreateTopicRequest(p.get("name"));
                if ("true".equals(p.get("fifo"))) r.setAttributes(Collections.singletonMap("FifoTopic", "true"));
                return new Main.Result().with("arn", sns.createTopic(r).getTopicArn());
            }
            case "deleteTopic":
                sns.deleteTopic(p.get("topic"));
                return new Main.Result();
            case "listTopics": {
                List<String> names = new ArrayList<>();
                String token = null;
                do {
                    ListTopicsResult o = sns.listTopics(token);
                    for (Topic t : o.getTopics()) {
                        String n = t.getTopicArn().substring(t.getTopicArn().lastIndexOf(':') + 1);
                        if (n.startsWith(p.get("prefix"))) names.add(n);
                    }
                    token = o.getNextToken();
                } while (token != null && !token.isEmpty());
                Collections.sort(names);
                return new Main.Result().with("count", String.valueOf(names.size()), "names", String.join(",", names));
            }
            case "getTopicAttributes":
                return attrs(sns.getTopicAttributes(p.get("topic")).getAttributes());
            case "subscribe": {
                SubscribeRequest r = new SubscribeRequest(p.get("topic"), p.get("protocol"), p.get("endpoint")).withReturnSubscriptionArn(true);
                if (p.containsKey("attrs")) r.setAttributes(Main.pairs(p.get("attrs")));
                return new Main.Result().with("arn", sns.subscribe(r).getSubscriptionArn());
            }
            case "unsubscribe":
                sns.unsubscribe(p.get("sub"));
                return new Main.Result();
            case "listSubscriptions": {
                List<Subscription> subs = sns.listSubscriptionsByTopic(p.get("topic")).getSubscriptions();
                return new Main.Result().with("count", String.valueOf(subs.size()),
                    "endpoints", subs.stream().map(Subscription::getEndpoint).sorted().collect(Collectors.joining(",")),
                    "protocols", subs.stream().map(Subscription::getProtocol).distinct().sorted().collect(Collectors.joining(",")));
            }
            case "getSubscriptionAttributes":
                return attrs(sns.getSubscriptionAttributes(p.get("sub")).getAttributes());
            case "setSubscriptionAttributes":
                for (Map.Entry<String, String> e : Main.pairs(p.get("attrs")).entrySet())
                    sns.setSubscriptionAttributes(p.get("sub"), e.getKey(), e.getValue());
                return new Main.Result();
            case "publish": {
                PublishRequest r = new PublishRequest(p.get("topic"), p.get("message"));
                if (p.containsKey("subject")) r.setSubject(p.get("subject"));
                if (p.containsKey("msgattrs")) r.setMessageAttributes(snsAttrs(p.get("msgattrs")));
                if (p.containsKey("group")) r.setMessageGroupId(p.get("group"));
                if (p.containsKey("dedup")) r.setMessageDeduplicationId(p.get("dedup"));
                return new Main.Result().with("messageId", sns.publish(r).getMessageId());
            }
            case "publishBatch": {
                String[] messages = p.get("messages").split(",");
                List<PublishBatchRequestEntry> entries = new ArrayList<>();
                for (int i = 0; i < messages.length; i++) entries.add(new PublishBatchRequestEntry().withId(String.valueOf(i)).withMessage(messages[i]));
                PublishBatchResult o = sns.publishBatch(new PublishBatchRequest().withTopicArn(p.get("topic")).withPublishBatchRequestEntries(entries));
                return batch(o.getSuccessful().size(), o.getFailed().size());
            }
            default:
                throw new IllegalStateException("operation " + op + " is not implemented");
        }
    }
}
