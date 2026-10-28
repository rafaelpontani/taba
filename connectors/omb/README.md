# Taba driver for the OpenMessaging Benchmark

A driver for the [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) (OMB) written on the Java
connector (`connectors/java`). Java 11. The OMB commit it is built against is in `OMB_VERSION`.

- An OMB topic is a Taba **distributor**; each subscription is a **queue** `<topic>-<subscription>` that the
  distributor copies every publication to. Consumers of one subscription compete for its messages.
- The OMB partitions are not partitions in Taba. The message key is the `groupId`: with `ordering: per-key` the queue is
  FIFO and each key is delivered in order; with `ordering: none` (default) the queue is ordinary and the group only
  serves fair delivery.
- The OMB asks the driver for the publish time, so the producer writes its clock in the first 8 bytes of the body
  and the consumer removes it. The body in the server is 8 bytes larger than the OMB payload.

```
connectors/omb/build.sh   # OMB driver-api + the connector + the driver, tests, connectors/omb/target/driver-taba-all.jar
connectors/omb/test.sh    # one Taba node in Docker, then the unit and the integration tests of the driver
```

Both run in a container (`maven:3.9-eclipse-temurin-11`), so only Docker is needed. To use the driver in an OMB
checkout, copy `target/driver-taba-all.jar` to its `lib/` and `driver-taba.yaml` to its `driver-taba/` folder.
