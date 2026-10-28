package io.github.rafaelpontani.taba;

/** The {@code changeVisibility} response. */
public final class VisibilityResponse {
    private final String id;
    private final String invisibleUntil;
    private final String raw;

    VisibilityResponse(String id, String invisibleUntil, String raw) {
        this.id = id;
        this.invisibleUntil = invisibleUntil;
        this.raw = raw;
    }

    public String getId() {
        return id;
    }

    public String getInvisibleUntil() {
        return invisibleUntil;
    }

    public String getRaw() {
        return raw;
    }
}
