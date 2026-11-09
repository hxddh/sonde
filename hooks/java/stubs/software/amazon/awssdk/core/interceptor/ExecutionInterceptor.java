// compile-only stand-in for the AWS SDK's interface: never in a jar
package software.amazon.awssdk.core.interceptor;

import software.amazon.awssdk.http.SdkHttpRequest;

public interface ExecutionInterceptor {
    default SdkHttpRequest modifyHttpRequest(Context.ModifyHttpRequest context, ExecutionAttributes attributes) {
        return context.httpRequest();
    }
}
