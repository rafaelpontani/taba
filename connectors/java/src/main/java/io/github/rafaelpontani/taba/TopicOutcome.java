package io.github.rafaelpontani.taba;

/** What {@code PUT /topics/{topic}} did. */
public enum TopicOutcome {
    /** It did not exist ({@code 201}). */
    CREATED,
    /** It existed: the mutable fields are now the declared ones, or already were ({@code 200}). */
    UPDATED,
    /** It was removed and created again with a new {@code id} ({@code 201} with {@code Taba-Topic-Recreated: true}). */
    RECREATED
}
