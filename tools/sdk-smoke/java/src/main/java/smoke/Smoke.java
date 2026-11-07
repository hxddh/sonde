// AWS SDK for Java v2 through `sonde run`: configured from the environment.
package smoke;

import java.io.InputStream;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Random;

import software.amazon.awssdk.core.sync.RequestBody;
import software.amazon.awssdk.services.s3.S3Client;
import software.amazon.awssdk.services.s3.model.*;
import software.amazon.awssdk.services.s3.presigner.S3Presigner;

public class Smoke {
    static void check(boolean ok, String what) {
        if (!ok) throw new IllegalStateException(what);
    }

    public static void main(String[] args) throws Exception {
        String bucket = System.getenv().getOrDefault("SMOKE_BUCKET", "sonde-smoke");
        S3Client s3 = S3Client.builder().forcePathStyle(true).build();
        try {
            s3.createBucket(b -> b.bucket(bucket));
        } catch (BucketAlreadyOwnedByYouException | BucketAlreadyExistsException e) {
            // fine
        }
        String p = "java/";
        s3.putObject(b -> b.bucket(bucket).key(p + "small.txt"), RequestBody.fromString("hello from java"));
        byte[] big = new byte[12 << 20];
        new Random(7).nextBytes(big);
        String upload = s3.createMultipartUpload(b -> b.bucket(bucket).key(p + "big.bin")).uploadId();
        List<CompletedPart> parts = new ArrayList<>();
        int part = 5 << 20;
        for (int i = 0, n = 1; i < big.length; i += part, n++) {
            final int num = n;
            byte[] chunk = Arrays.copyOfRange(big, i, Math.min(big.length, i + part));
            String etag = s3.uploadPart(b -> b.bucket(bucket).key(p + "big.bin").uploadId(upload).partNumber(num),
                    RequestBody.fromBytes(chunk)).eTag();
            parts.add(CompletedPart.builder().partNumber(num).eTag(etag).build());
        }
        s3.completeMultipartUpload(b -> b.bucket(bucket).key(p + "big.bin").uploadId(upload)
                .multipartUpload(m -> m.parts(parts)));
        byte[] got = s3.getObjectAsBytes(b -> b.bucket(bucket).key(p + "big.bin").range("bytes=100-199")).asByteArray();
        check(Arrays.equals(got, Arrays.copyOfRange(big, 100, 200)), "ranged GET");
        check(s3.headObject(b -> b.bucket(bucket).key(p + "big.bin")).contentLength() == big.length, "HEAD length");
        check(s3.listObjectsV2(b -> b.bucket(bucket).prefix(p)).contents().size() >= 2, "LIST");
        s3.copyObject(b -> b.sourceBucket(bucket).sourceKey(p + "small.txt").destinationBucket(bucket).destinationKey(p + "copy.txt"));
        try (S3Presigner presigner = S3Presigner.builder()
                .serviceConfiguration(software.amazon.awssdk.services.s3.S3Configuration.builder().pathStyleAccessEnabled(true).build())
                .build()) {
            URI url = presigner.presignGetObject(b -> b.signatureDuration(Duration.ofMinutes(1))
                    .getObjectRequest(g -> g.bucket(bucket).key(p + "copy.txt"))).url().toURI();
            try (InputStream in = url.toURL().openStream()) {
                check(new String(in.readAllBytes(), StandardCharsets.UTF_8).equals("hello from java"), "presigned GET");
            }
        }
        List<ObjectIdentifier> ids = new ArrayList<>();
        for (String k : List.of("small.txt", "big.bin", "copy.txt")) ids.add(ObjectIdentifier.builder().key(p + k).build());
        DeleteObjectsResponse d = s3.deleteObjects(b -> b.bucket(bucket).delete(x -> x.objects(ids)));
        check(d.errors().isEmpty(), "delete objects: " + d.errors());
        System.out.println("java v2 ok");
    }
}
