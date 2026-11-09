// compile-only stand-in for the AWS SDK's interface: never in a jar
package software.amazon.awssdk.core.interceptor;

import software.amazon.awssdk.http.SdkHttpRequest;

public final class Context {
    private Context() {}

    public interface ModifyHttpRequest {
        SdkHttpRequest httpRequest();
    }
}
