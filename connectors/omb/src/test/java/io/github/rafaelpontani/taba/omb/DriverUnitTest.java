package io.github.rafaelpontani.taba.omb;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.rafaelpontani.taba.TabaClient;
import io.github.rafaelpontani.taba.TabaConfig;
import java.util.Optional;
import org.junit.jupiter.api.Test;

class DriverUnitTest {
    /// UC: UC-CNT-039
    @Test
    void theStampComesOutOfTheBodyWithThePayloadIntact() {
        byte[] payload = {1, 2, 3, (byte) 0xff, 0};
        byte[] body = Stamp.wrap(1_700_000_000_123L, payload);
        assertEquals(Stamp.SIZE + payload.length, body.length);
        assertEquals(1_700_000_000_123L, Stamp.millis(body));
        assertArrayEquals(payload, Stamp.payload(body));
    }

    private static TabaBenchmarkProducer producer(boolean perKey, int groups) {
        TabaClient client = new TabaClient(TabaConfig.of("http://localhost:1"));
        return new TabaBenchmarkProducer(client, "t", perKey, groups, 1);
    }

    /// UC: UC-CNT-038
    @Test
    void theKeyIsTheGroupWhenThereIsOne() {
        assertEquals("k", producer(false, 4).groupOf(Optional.of("k")));
        assertEquals("k", producer(true, 4).groupOf(Optional.of("k")));
    }

    /// UC: UC-CNT-038
    @Test
    void withoutKeyThereIsNoGroupInAnOrdinaryQueue() {
        assertNull(producer(false, 4).groupOf(Optional.empty()));
    }

    /// UC: UC-CNT-038
    @Test
    void withoutKeyPerKeyRotatesOverThePartitionsAsGroups() {
        TabaBenchmarkProducer p = producer(true, 3);
        StringBuilder seen = new StringBuilder();
        for (int i = 0; i < 7; i++) {
            seen.append(p.groupOf(Optional.empty())).append(' ');
        }
        assertEquals("p0 p1 p2 p0 p1 p2 p0 ", seen.toString());
    }

    /// UC: UC-CNT-040
    @Test
    void theConfigurationIsValidated() {
        TabaDriverConfig ok = new TabaDriverConfig();
        ok.validate();
        assertTrue(!ok.isPerKey());

        TabaDriverConfig badOrdering = new TabaDriverConfig();
        badOrdering.ordering = "ordered";
        assertThrows(IllegalArgumentException.class, badOrdering::validate);

        TabaDriverConfig badWait = new TabaDriverConfig();
        badWait.consumer.waitTime = 21;
        assertThrows(IllegalArgumentException.class, badWait::validate);

        TabaDriverConfig badInFlight = new TabaDriverConfig();
        badInFlight.producer.inFlight = 0;
        assertThrows(IllegalArgumentException.class, badInFlight::validate);
    }

    /// UC: UC-CNT-036
    @Test
    void theTopicPrefixIsOmb() {
        assertEquals("omb", new TabaBenchmarkDriver().getTopicNamePrefix());
    }
}
