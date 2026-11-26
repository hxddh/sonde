//! Core services against live S3 implementations (see `tests/integration`).

mod common;

use bytes::Bytes;
use sonde_core::services::{browse, du, exec, mpu, versions};

#[tokio::test]
async fn browse_mpu_versions_du() {
    for t in common::targets() {
        let s = &t.sonde;
        let c = s.client(&t.name).unwrap();
        let b = common::bucket_name("core");
        c.create_bucket(&b, false)
            .await
            .unwrap_or_else(|e| panic!("{}: create bucket: {e}", t.name));
        c.put_bucket_versioning(&b, "Enabled").await.unwrap();
        for i in 0..25 {
            c.put_object(
                &b,
                &format!("logs/2024/{i:03}.txt"),
                Bytes::from(vec![b'x'; 100 + i]),
                &[],
            )
            .await
            .unwrap();
        }
        c.put_object(&b, "img/a.png", Bytes::from(vec![0u8; 5000]), &[])
            .await
            .unwrap();
        c.put_object(&b, "img/a.png", Bytes::from(vec![1u8; 7000]), &[])
            .await
            .unwrap();
        c.delete_object(&b, "img/a.png", None).await.unwrap();

        // paging and delimiters
        let root = browse::list_page(s, &t.name, &b, "", Some("/"), None, None)
            .await
            .unwrap();
        let names: Vec<&str> = root.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"logs/"), "{}: {names:?}", t.name);
        let mut token = None;
        let mut seen = 0;
        loop {
            let p = browse::list_page(s, &t.name, &b, "logs/2024/", Some("/"), token, Some(10))
                .await
                .unwrap();
            seen += p.entries.len();
            if !p.is_truncated {
                break;
            }
            token = p.next_token;
        }
        assert_eq!(seen, 25, "{}", t.name);

        let d = browse::object_detail(s, &t.name, &b, "logs/2024/001.txt", None)
            .await
            .unwrap();
        assert_eq!(d.size, Some(101));
        std::assert_matches!(d.etag.unwrap().kind, s3raw::etag::EtagKind::Md5 { .. });

        // versions and delete markers
        let tl = versions::timeline(s, &t.name, &b, "img/a.png")
            .await
            .unwrap();
        assert!(tl.deleted, "{}: {:?}", t.name, tl);
        assert_eq!(tl.versions.len(), 3);
        let v = versions::versioning(s, &t.name, &b).await.unwrap();
        assert_eq!(v.status, "Enabled");
        let plan = versions::plan_restore(s, &t.name, &b, "img/a.png")
            .await
            .unwrap();
        exec::execute(s, &plan.id, &b).await.unwrap();
        assert!(
            !versions::timeline(s, &t.name, &b, "img/a.png")
                .await
                .unwrap()
                .deleted
        );

        // an incomplete upload
        let up = c.create_multipart_upload(&b, "big.bin", &[]).await.unwrap();
        c.upload_part(
            &b,
            "big.bin",
            &up,
            1,
            Bytes::from(vec![7u8; 5 * 1024 * 1024]),
            &[],
        )
        .await
        .unwrap();
        let id = mpu::scan(s, &t.name, &b, "");
        let r: mpu::MpuReport = serde_json::from_value(s.jobs().wait(&id).await.unwrap()).unwrap();
        let scanned = r.uploads.clone();
        assert_eq!(r.total_uploads, 1, "{}", t.name);
        assert_eq!(r.uploads[0].bytes, 5 * 1024 * 1024);
        assert_eq!(r.lifecycle.status, mpu::LifecycleStatus::Missing);

        // capacity
        c.delete_object(&b, "logs/2024/000.txt", None)
            .await
            .unwrap();
        let id = du::scan(s, &t.name, &b, "", true);
        let r: du::DuReport = serde_json::from_value(s.jobs().wait(&id).await.unwrap()).unwrap();
        assert_eq!(r.multipart.bytes, 5 * 1024 * 1024);
        assert!(r.noncurrent.objects >= 2, "{}: {r:?}", t.name);
        assert!(r.delete_markers >= 1);

        // abort through the guard, then lifecycle, from the rows the scan
        // showed, as the app's are (one upload: a plain confirm, nothing
        // typed); held to what the bucket holds when it runs
        let plan = mpu::plan_abort(s, &t.name, &b, &scanned);
        assert!(!plan.typed);
        let o = exec::execute(s, &plan.id, "").await.unwrap();
        assert_eq!(o.succeeded, 1);
        // a lifecycle replaces the bucket's rules: typed, and refused when wrong
        let plan = mpu::plan_lifecycle_rule(s, &t.name, &b, "", 7)
            .await
            .unwrap();
        assert!(plan.typed);
        assert!(exec::execute(s, &plan.id, "wrong").await.is_err());
        let o = exec::execute(s, &plan.id, &b).await.unwrap();
        if t.name == "minio" {
            // MinIO takes no AbortIncompleteMultipartUpload in a lifecycle
            // (its internal/bucket/lifecycle/rule.go): the store's refusal is
            // the outcome, said, and the bucket is left as it was
            assert_eq!(o.succeeded, 0, "{}: {o:?}", t.name);
            assert!(!o.errors.is_empty(), "{}: {o:?}", t.name);
        } else {
            assert_eq!(o.succeeded, 1, "{}: {:?}", t.name, o.errors);
        }
        // each run is on record twice: before its first request, then its outcome
        let audit = s.guard().audit_entries(10);
        assert_eq!(audit.len(), 6);
        assert_eq!(audit.iter().filter(|e| e.stage.is_some()).count(), 3);
        cleanup(&c, &b).await;
    }
}

pub async fn cleanup(c: &s3raw::S3Client, b: &str) {
    loop {
        let v = c
            .list_object_versions(b, None, None, None, None)
            .await
            .unwrap();
        let mut objs: Vec<(String, Option<String>)> = v
            .version
            .iter()
            .map(|v| (v.key.clone(), v.version_id.clone()))
            .collect();
        objs.extend(
            v.delete_marker
                .iter()
                .map(|d| (d.key.clone(), d.version_id.clone())),
        );
        if objs.is_empty() {
            break;
        }
        c.delete_objects(b, &objs, true).await.unwrap();
    }
    let _ = c.delete_bucket(b).await;
}
