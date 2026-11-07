// AWS SDK for Go v2 through `sonde run`: configured from the environment.
package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"fmt"
	"io"
	"net/http"
	"os"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/feature/s3/manager"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/aws-sdk-go-v2/service/s3/types"
)

func check(err error, what string) {
	if err != nil {
		fmt.Fprintf(os.Stderr, "%s: %v\n", what, err)
		os.Exit(1)
	}
}

func main() {
	ctx := context.Background()
	bucket := os.Getenv("SMOKE_BUCKET")
	if bucket == "" {
		bucket = "sonde-smoke"
	}
	cfg, err := config.LoadDefaultConfig(ctx)
	check(err, "config")
	c := s3.NewFromConfig(cfg, func(o *s3.Options) { o.UsePathStyle = true })
	if _, err := c.CreateBucket(ctx, &s3.CreateBucketInput{Bucket: &bucket}); err != nil {
		var owned *types.BucketAlreadyOwnedByYou
		var exists *types.BucketAlreadyExists
		if !(asErr(err, &owned) || asErr(err, &exists)) {
			check(err, "create bucket")
		}
	}
	p := "go/"
	_, err = c.PutObject(ctx, &s3.PutObjectInput{Bucket: &bucket, Key: aws.String(p + "small.txt"), Body: bytes.NewReader([]byte("hello from go"))})
	check(err, "put")
	big := make([]byte, 12<<20)
	rand.Read(big)
	_, err = manager.NewUploader(c, func(u *manager.Uploader) { u.PartSize = 5 << 20 }).Upload(ctx, &s3.PutObjectInput{Bucket: &bucket, Key: aws.String(p + "big.bin"), Body: bytes.NewReader(big)})
	check(err, "multipart upload")
	g, err := c.GetObject(ctx, &s3.GetObjectInput{Bucket: &bucket, Key: aws.String(p + "big.bin"), Range: aws.String("bytes=100-199")})
	check(err, "ranged get")
	got, _ := io.ReadAll(g.Body)
	if !bytes.Equal(got, big[100:200]) {
		check(fmt.Errorf("content differs"), "ranged get")
	}
	h, err := c.HeadObject(ctx, &s3.HeadObjectInput{Bucket: &bucket, Key: aws.String(p + "big.bin")})
	check(err, "head")
	if *h.ContentLength != int64(len(big)) {
		check(fmt.Errorf("length %d", *h.ContentLength), "head")
	}
	_, err = c.ListObjectsV2(ctx, &s3.ListObjectsV2Input{Bucket: &bucket, Prefix: aws.String(p)})
	check(err, "list")
	_, err = c.CopyObject(ctx, &s3.CopyObjectInput{Bucket: &bucket, Key: aws.String(p + "copy.txt"), CopySource: aws.String(bucket + "/" + p + "small.txt")})
	check(err, "copy")
	ps, err := s3.NewPresignClient(c).PresignGetObject(ctx, &s3.GetObjectInput{Bucket: &bucket, Key: aws.String(p + "copy.txt")})
	check(err, "presign")
	resp, err := http.Get(ps.URL)
	check(err, "presigned get")
	body, _ := io.ReadAll(resp.Body)
	if string(body) != "hello from go" {
		check(fmt.Errorf("got %q", body), "presigned get")
	}
	var objs []types.ObjectIdentifier
	for _, k := range []string{"small.txt", "big.bin", "copy.txt"} {
		objs = append(objs, types.ObjectIdentifier{Key: aws.String(p + k)})
	}
	d, err := c.DeleteObjects(ctx, &s3.DeleteObjectsInput{Bucket: &bucket, Delete: &types.Delete{Objects: objs}})
	check(err, "delete objects")
	if len(d.Errors) > 0 {
		check(fmt.Errorf("%d errors", len(d.Errors)), "delete objects")
	}
	fmt.Println("go v2 ok")
}

func asErr[T error](err error, target *T) bool {
	for e := err; e != nil; {
		if t, ok := e.(T); ok {
			*target = t
			return true
		}
		u, ok := e.(interface{ Unwrap() error })
		if !ok {
			return false
		}
		e = u.Unwrap()
	}
	return false
}
