package io.github.rafaelpontani.taba;

/**
 * Who serializes the object of {@code publishJson} and deserializes the body of the typed {@code consume}.
 * The JDK has no JSON library and the connector has no dependencies, so the application plugs its own:
 *
 * <pre>{@code
 * ObjectMapper mapper = new ObjectMapper();
 * TabaConfig config = TabaConfig.of("http://localhost:8080").jsonSerializer(new TabaJsonSerializer() {
 *     public String serialize(Object value) throws Exception { return mapper.writeValueAsString(value); }
 *     public <T> T deserialize(String json, Class<T> type) throws Exception { return mapper.readValue(json, type); }
 * });
 * }</pre>
 */
public interface TabaJsonSerializer {
    String serialize(Object value) throws Exception;

    <T> T deserialize(String json, Class<T> type) throws Exception;
}
