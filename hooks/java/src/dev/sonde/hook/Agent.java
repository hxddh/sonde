// sonde: names the place in a Java program that sent each S3 request.
//
// Loaded with JAVA_TOOL_OPTIONS=-javaagent by `sonde run` (unless --no-callers)
// and the app's "Name the code", only then. It adds the interceptor beside it
// (sonde-caller.jar) to the class path, and only when the AWS SDK for Java 2
// is on that class path too: a program that loads the SDK from a class
// loader of its own (a Spring Boot jar) is left as it is. Nothing here may
// stop the program.
package dev.sonde.hook;

import java.io.File;
import java.lang.instrument.Instrumentation;
import java.util.jar.JarFile;

public final class Agent {
    private Agent() {}

    public static void premain(String args, Instrumentation inst) {
        try {
            ClassLoader system = ClassLoader.getSystemClassLoader();
            if (system.getResource("software/amazon/awssdk/core/interceptor/ExecutionInterceptor.class") == null) {
                return;
            }
            File self = new File(Agent.class.getProtectionDomain().getCodeSource().getLocation().toURI());
            File caller = new File(self.getParentFile(), "sonde-caller.jar");
            if (caller.isFile()) {
                inst.appendToSystemClassLoaderSearch(new JarFile(caller));
            }
        } catch (Throwable t) {
            // the program runs as it would without sonde
        }
    }
}
