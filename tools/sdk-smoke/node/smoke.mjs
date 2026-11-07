// AWS SDK for JavaScript v3 through `sonde run`: configured from the environment.
import { randomBytes } from "node:crypto";
import {
  S3Client, CreateBucketCommand, PutObjectCommand, GetObjectCommand, HeadObjectCommand,
  ListObjectsV2Command, CopyObjectCommand, DeleteObjectsCommand,
} from "@aws-sdk/client-s3";
import { Upload } from "@aws-sdk/lib-storage";
import { getSignedUrl } from "@aws-sdk/s3-request-presigner";

const Bucket = process.env.SMOKE_BUCKET ?? "sonde-smoke";
const s3 = new S3Client({ forcePathStyle: true });
const assert = (c, m) => { if (!c) throw new Error(m); };
try { await s3.send(new CreateBucketCommand({ Bucket })); } catch (e) {
  if (!["BucketAlreadyOwnedByYou", "BucketAlreadyExists"].includes(e.name)) throw e;
}
const p = "node/";
await s3.send(new PutObjectCommand({ Bucket, Key: p + "small.txt", Body: "hello from js" }));
const big = randomBytes(12 * 1024 * 1024);
await new Upload({ client: s3, params: { Bucket, Key: p + "big.bin", Body: big }, partSize: 5 * 1024 * 1024 }).done();
const ranged = await s3.send(new GetObjectCommand({ Bucket, Key: p + "big.bin", Range: "bytes=100-199" }));
assert(Buffer.from(await ranged.Body.transformToByteArray()).equals(big.subarray(100, 200)), "ranged GET");
const head = await s3.send(new HeadObjectCommand({ Bucket, Key: p + "big.bin" }));
assert(head.ContentLength === big.length, "HEAD length");
const list = await s3.send(new ListObjectsV2Command({ Bucket, Prefix: p }));
assert(list.Contents.some((o) => o.Key === p + "big.bin"), "LIST");
await s3.send(new CopyObjectCommand({ Bucket, Key: p + "copy.txt", CopySource: `${Bucket}/${p}small.txt` }));
const url = await getSignedUrl(s3, new GetObjectCommand({ Bucket, Key: p + "copy.txt" }), { expiresIn: 60 });
assert((await (await fetch(url)).text()) === "hello from js", "presigned GET");
const del = await s3.send(new DeleteObjectsCommand({ Bucket, Delete: { Objects: ["small.txt", "big.bin", "copy.txt"].map((k) => ({ Key: p + k })) } }));
assert(!del.Errors?.length, JSON.stringify(del.Errors));
console.log("js v3 ok");
