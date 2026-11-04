import java.util.Map;

/** What the engine needs from an SDK flavour (software.amazon.awssdk v2 or com.amazonaws v1). */
interface Ops {
    /** Runs one operation. */
    Main.Result run(String op, Map<String, String> p, Map<String, Main.Result> vars) throws Exception;

    /** The AWS error code of a service error, or null if the exception is not an answer from the service. */
    String errorCode(Throwable t);

    String sdkName();
}
