"""boto3 through `sonde run`: configured only from the environment."""
import io, os, sys, urllib.request
import boto3
from boto3.s3.transfer import TransferConfig
from botocore.config import Config

bucket = os.environ.get("SMOKE_BUCKET", "sonde-smoke")
s3 = boto3.client("s3", config=Config(s3={"addressing_style": "path"}))
try:
    s3.create_bucket(Bucket=bucket)
except s3.exceptions.BucketAlreadyOwnedByYou:
    pass
except s3.exceptions.BucketAlreadyExists:
    pass
p = "python/"
s3.put_object(Bucket=bucket, Key=p + "small.txt", Body=b"hello from boto3")
big = os.urandom(12 * 1024 * 1024)
s3.upload_fileobj(io.BytesIO(big), bucket, p + "big.bin",
                  Config=TransferConfig(multipart_threshold=5 * 1024 * 1024, multipart_chunksize=5 * 1024 * 1024))
got = s3.get_object(Bucket=bucket, Key=p + "big.bin", Range="bytes=100-199")["Body"].read()
assert got == big[100:200], "ranged GET"
assert s3.head_object(Bucket=bucket, Key=p + "big.bin")["ContentLength"] == len(big), "HEAD length"
keys = [o["Key"] for o in s3.list_objects_v2(Bucket=bucket, Prefix=p)["Contents"]]
assert p + "small.txt" in keys and p + "big.bin" in keys, keys
s3.copy_object(Bucket=bucket, Key=p + "copy.txt", CopySource={"Bucket": bucket, "Key": p + "small.txt"})
url = s3.generate_presigned_url("get_object", Params={"Bucket": bucket, "Key": p + "copy.txt"}, ExpiresIn=60)
assert urllib.request.urlopen(url).read() == b"hello from boto3", "presigned GET"
r = s3.delete_objects(Bucket=bucket, Delete={"Objects": [{"Key": k} for k in (p + "small.txt", p + "big.bin", p + "copy.txt")]})
assert not r.get("Errors"), r.get("Errors")
print("boto3 ok")
