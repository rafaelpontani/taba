// A queue, a distributor and a token of Taba, and the data sources that read them. The configuration (pulumi config)
// switches the pieces the acceptance scenarios change: prefix, maxDeliveries, volatile, forceDestroy and adopt.
import * as pulumi from "@pulumi/pulumi";
import * as taba from "@rafaelpontani/pulumi-taba";

const config = new pulumi.Config();
const prefix = config.require("prefix");
const maxDeliveries = config.getNumber("maxDeliveries") ?? 5;
const volatile = config.getBoolean("volatile") ?? false;
const forceDestroy = config.getBoolean("forceDestroy") ?? false;
const adopt = config.getBoolean("adopt") ?? false;

const orders = new taba.Queue("orders", {
    name: `${prefix}-orders`,
    maxDeliveries,
    volatile,
    forceDestroy,
    tags: { team: "billing" },
});

const events = new taba.Distributor("events", {
    name: `${prefix}-events`,
    prefixes: [`${prefix}-orders`],
});

const app = new taba.Token("app", {
    name: `${prefix}-app`,
    permissions: ["publish", "consume"],
    topicPrefixes: [prefix],
});

// A queue that an application may have created first: createOrUpdate takes it over.
if (adopt) {
    new taba.Queue("adopted", { name: `${prefix}-adopted`, maxDeliveries: 3, createOrUpdate: true });
}

const found = taba.getTopicOutput({ name: orders.name });
const listed = taba.getTopicsOutput({ nameContains: orders.name.apply(() => prefix) });

export const queueName = orders.name;
export const effectiveTtl = found.apply((t) => t.effectiveTtl);
export const queueNames = listed.apply((t) => t.names);
export const distributorName = events.name;
export const tokenId = app.id;
export const tokenSecret = app.secret;
