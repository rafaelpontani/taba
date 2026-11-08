"""A queue, a distributor and a token of Taba, and the data sources that read them. The configuration (pulumi config)
switches the pieces the acceptance scenarios change: prefix, maxDeliveries, volatile, forceDestroy and adopt."""
import pulumi
import pulumi_taba as taba

config = pulumi.Config()
prefix = config.require("prefix")
max_deliveries = config.get_int("maxDeliveries") or 5
volatile = config.get_bool("volatile") or False
force_destroy = config.get_bool("forceDestroy") or False
adopt = config.get_bool("adopt") or False

orders = taba.Queue(
    "orders",
    name=f"{prefix}-orders",
    max_deliveries=max_deliveries,
    volatile=volatile,
    force_destroy=force_destroy,
    tags={"team": "billing"},
)

events = taba.Distributor("events", name=f"{prefix}-events", prefixes=[f"{prefix}-orders"])

app = taba.Token(
    "app",
    name=f"{prefix}-app",
    permissions=["publish", "consume"],
    topic_prefixes=[prefix],
)

# A queue that an application may have created first: create_or_update takes it over.
if adopt:
    taba.Queue("adopted", name=f"{prefix}-adopted", max_deliveries=3, create_or_update=True)

found = taba.get_topic_output(name=orders.name)
listed = taba.get_topics_output(name_contains=orders.name.apply(lambda _: prefix))

pulumi.export("queueName", orders.name)
pulumi.export("effectiveTtl", found.effective_ttl)
pulumi.export("queueNames", listed.names)
pulumi.export("distributorName", events.name)
pulumi.export("tokenId", app.id)
pulumi.export("tokenSecret", app.secret)
