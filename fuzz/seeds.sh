#!/usr/bin/env sh
# Fills fuzz/corpus/<target> with real inputs to start from: the decoders'
# samples, the sessions the tests read, and a few lines of each log.
set -eu
cd "$(dirname "$0")"
samples=../crates/decoders/samples
seed() { mkdir -p "corpus/$1"; }
copy() { t="$1"; shift; seed "$t"; for f in "$@"; do cp "$f" "corpus/$t/$(echo "$f" | sed 's#^\.\./##; s#/#_#g')"; done; }
copy iceberg_metadata $(find $samples/iceberg -name '*.metadata.json')
copy iceberg_avro $(find $samples/iceberg -name '*.avro')
copy slatedb $(find $samples/slatedb -type f)
copy lakefs $(find $samples/lakefs/sample/_lakefs -type f)
copy woodpecker $(find $samples/woodpecker -name '*.blk')
copy session_lines ../crates/proxy/tests/fixtures/*.sonde
# (what follows "79a5 lake [", which the target puts before each input)
seed import_access_log
printf '%s\n' '06/Feb/2031:00:00:38 +0000] 192.0.2.3 - 3E57 REST.GET.OBJECT t/a "GET /lake/t/a HTTP/1.1" 200 - 4096 4096 12 10 "-" "boto3/1.34" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -' \
  > corpus/import_access_log/get
# (what follows {"Records":)
seed import_cloudtrail
printf '%s' '[{"eventVersion":"1.09","eventSource":"s3.amazonaws.com","eventName":"GetObject","eventTime":"2031-02-06T00:00:38Z","awsRegion":"us-east-1","sourceIPAddress":"192.0.2.3","userAgent":"[aws-sdk-java/2.21]","requestParameters":{"bucketName":"lake","key":"t/a","Host":"lake.s3.amazonaws.com"},"responseElements":null,"additionalEventData":{"bytesTransferredIn":0,"bytesTransferredOut":4096},"requestID":"3E57","readOnly":true,"resources":[{"type":"AWS::S3::Object","ARN":"arn:aws:s3:::lake/t/a"}]}]}' \
  > corpus/import_cloudtrail/get
# (what follows {"log":{"entries":)
seed import_har
printf '%s' '[{"startedDateTime":"2031-02-06T00:00:38.000Z","time":12,"request":{"method":"GET","url":"https://lake.s3.amazonaws.com/t/a?x-id=GetObject","headers":[{"name":"Host","value":"lake.s3.amazonaws.com"}]},"response":{"status":200,"headers":[{"name":"Content-Length","value":"4096"},{"name":"x-amz-request-id","value":"3E57"}],"content":{"size":4096}},"timings":{"wait":10}}]}}' \
  > corpus/import_har/get
seed s3_xml
printf '%s' '<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>lake</Name><Prefix>t/</Prefix><KeyCount>1</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated><Contents><Key>t/a</Key><LastModified>2031-02-06T00:00:38.000Z</LastModified><ETag>"e"</ETag><Size>4096</Size><StorageClass>STANDARD</StorageClass></Contents></ListBucketResult>' \
  > corpus/s3_xml/list
printf '%s' '<Error><Code>SlowDown</Code><Message>Please reduce your request rate.</Message><RequestId>3E57</RequestId></Error>' \
  > corpus/s3_xml/error
for d in corpus/*; do echo "$d: $(ls "$d" | wc -l) seeds"; done
