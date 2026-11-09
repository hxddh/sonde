// compile-only stand-in for the AWS SDK's interface: never in a jar (its
// builder is reached by reflection, as its generic methods erase otherwise)
package software.amazon.awssdk.http;

public interface SdkHttpRequest {
    String host();

    int port();
}
