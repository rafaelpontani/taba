package io.github.rafaelpontani.taba;

import java.util.Collections;
import java.util.List;

/** A page of results: the items and the cursor of the next one, if any. */
public final class Page<T> {
    private final List<T> items;
    private final String nextCursor;
    private final String raw;

    Page(List<T> items, String nextCursor, String raw) {
        this.items = Collections.unmodifiableList(items);
        this.nextCursor = nextCursor;
        this.raw = raw;
    }

    public List<T> getItems() {
        return items;
    }

    /** The cursor of the next page, or null on the last one. */
    public String getNextCursor() {
        return nextCursor;
    }

    public String getRaw() {
        return raw;
    }
}
