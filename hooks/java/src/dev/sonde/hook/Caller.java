// sonde: the AWS SDK for Java 2 interceptor (a global one, from
// software/amazon/awssdk/global/handlers/execution.interceptors) that adds
// x-sonde-caller: <file>:<line> <method> to each request to sonde, before it
// is signed: the first frame of the program's own code. sonde reads it and
// never forwards it to the store.
package dev.sonde.hook;

import java.net.URI;
import software.amazon.awssdk.core.interceptor.Context;
import software.amazon.awssdk.core.interceptor.ExecutionAttributes;
import software.amazon.awssdk.core.interceptor.ExecutionInterceptor;
import software.amazon.awssdk.http.SdkHttpRequest;

public final class Caller implements ExecutionInterceptor {
    private static final String[] NOT_OURS = {
        "java.", "javax.", "jdk.", "sun.", "com.sun.", "software.amazon.", "dev.sonde.", "kotlin.", "kotlinx.",
        "scala.", "org.apache.", "io.netty.", "reactor.", "org.reactivestreams.", "org.slf4j.", "com.fasterxml.",
        "org.springframework.", "org.junit.", "groovy.", "org.codehaus.groovy.", "clojure.",
    };

    private final String host;
    private final int port;

    public Caller() {
        String h = null;
        int p = -1;
        try {
            String e = System.getenv("AWS_ENDPOINT_URL");
            if (e != null && !e.isEmpty()) {
                URI u = URI.create(e);
                h = u.getHost();
                p = u.getPort() != -1 ? u.getPort() : ("https".equals(u.getScheme()) ? 443 : 80);
            }
        } catch (Throwable t) {
            h = null;
        }
        host = h;
        port = p;
    }

    @Override
    public SdkHttpRequest modifyHttpRequest(Context.ModifyHttpRequest context, ExecutionAttributes attributes) {
        SdkHttpRequest r = context.httpRequest();
        try {
            if (host == null || !toSonde(r)) {
                return r;
            }
            String place = place();
            return place == null ? r : withHeader(r, place);
        } catch (Throwable t) {
            return r;
        }
    }

    // toBuilder() and build() are generic in the SDK (their erased types are
    // its builder interfaces'): called through those public interfaces, not
    // compiled against them
    private static SdkHttpRequest withHeader(SdkHttpRequest r, String place) throws Exception {
        ClassLoader l = r.getClass().getClassLoader();
        Class<?> builder = Class.forName("software.amazon.awssdk.http.SdkHttpRequest$Builder", false, l);
        Object b = SdkHttpRequest.class.getMethod("toBuilder").invoke(r);
        builder.getMethod("putHeader", String.class, String.class).invoke(b, "x-sonde-caller", place);
        return (SdkHttpRequest) builder.getMethod("build").invoke(b);
    }

    // only requests to sonde carry the header: never one to another store
    private boolean toSonde(SdkHttpRequest r) {
        String h = r.host();
        int p = r.port();
        return p == port && h != null && (h.equals(host) || h.endsWith("." + host));
    }

    private static boolean ours(String cls) {
        for (String p : NOT_OURS) {
            if (cls.startsWith(p)) {
                return false;
            }
        }
        return true;
    }

    // the first frame of the program's own code, as its source file under its package
    static String place() {
        for (StackTraceElement f : new Throwable().getStackTrace()) {
            String cls = f.getClassName();
            if (!ours(cls) || f.getLineNumber() < 0) {
                continue;
            }
            int dot = cls.lastIndexOf('.');
            String dir = dot < 0 ? "" : cls.substring(0, dot).replace('.', '/') + "/";
            String file = f.getFileName() != null ? f.getFileName() : cls.substring(dot + 1) + ".java";
            String s = dir + file + ":" + f.getLineNumber() + " " + f.getMethodName();
            StringBuilder b = new StringBuilder();
            for (char c : s.toCharArray()) {
                b.append(c >= 0x20 && c < 0x7f ? c : '?');
            }
            return b.length() > 200 ? b.substring(0, 200) : b.toString();
        }
        return null;
    }
}
