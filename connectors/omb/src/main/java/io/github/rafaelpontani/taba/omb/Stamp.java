package io.github.rafaelpontani.taba.omb;

import java.nio.ByteBuffer;

/**
 * The publish time of the OMB: Taba carries no attribute besides the body, so the producer puts its clock
 * (milliseconds, big-endian) in the first {@value #SIZE} bytes of the body and the consumer takes it out. It is the
 * same reading of the producer's clock that the Kafka driver gets from the record's create time.
 */
final class Stamp {
    static final int SIZE = 8;

    private Stamp() {
    }

    static byte[] wrap(long millis, byte[] payload) {
        return ByteBuffer.allocate(SIZE + payload.length).putLong(millis).put(payload).array();
    }

    static long millis(byte[] body) {
        return ByteBuffer.wrap(body).getLong();
    }

    static byte[] payload(byte[] body) {
        byte[] payload = new byte[body.length - SIZE];
        System.arraycopy(body, SIZE, payload, 0, payload.length);
        return payload;
    }
}
