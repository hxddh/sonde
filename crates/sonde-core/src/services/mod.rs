//! The diagnoses, one module each.

pub mod browse;
pub mod du;
pub mod exec;
pub mod layout;
pub mod mpu;
pub mod versions;

use chrono::{DateTime, Utc};

/// Parses an S3 timestamp (`2024-01-01T00:00:00.000Z`, or RFC 1123 in headers).
pub fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_rfc2822(s))
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// The markers a paged listing was handed. A store that hands one back
/// again would be asked for the same pages forever: that is an error.
#[derive(Default)]
pub struct Pages(std::collections::HashSet<String>);

impl Pages {
    /// Takes the marker of the next page.
    pub fn next(&mut self, marker: impl std::fmt::Debug) -> Result<(), s3raw::S3Error> {
        if self.0.insert(format!("{marker:?}")) {
            Ok(())
        } else {
            Err(s3raw::S3Error::decode(format!(
                "the store handed back the page marker {marker:?} again: the listing would never end"
            )))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::Sonde;

    /// A sonde with endpoint `e` on a store that answers every request
    /// (its target) with 200 and `answer`'s body.
    pub(crate) async fn store(answer: fn(&str) -> String) -> (tempfile::TempDir, Arc<Sonde>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut c, _)) = l.accept().await {
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    loop {
                        let mut chunk = [0u8; 4096];
                        match c.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                        let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                            continue;
                        };
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        buf.drain(..end + 4);
                        let body = answer(head.split(' ').nth(1).unwrap_or(""));
                        let reply = format!(
                            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        if c.write_all(reply.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        let mut e = crate::EndpointConfig::new("e", "e", &url, "us-east-1", crate::Vendor::Other);
        e.path_style = true;
        e.credentials = crate::CredentialSource::Anonymous;
        s.add_transient_endpoint(e).unwrap();
        (dir, s)
    }

    /// `S3Client::list_all` on a store whose every page names the same
    /// next page: an error, not a listing asked for ever.
    #[tokio::test]
    async fn a_token_handed_back_again_ends_list_all() {
        let (_dir, s) = store(|_| {
            "<ListBucketResult><IsTruncated>true</IsTruncated>\
             <NextContinuationToken>t</NextContinuationToken>\
             <Contents><Key>k</Key><Size>1</Size></Contents></ListBucketResult>"
                .into()
        })
        .await;
        let c = s.client("e").unwrap();
        let listed = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            c.list_all("b", "", usize::MAX),
        )
        .await
        .expect("the listing ended");
        let e = listed.unwrap_err();
        assert!(e.message.contains("again"), "{e}");
    }

    #[test]
    fn a_marker_handed_back_again_ends_the_listing() {
        let mut p = super::Pages::default();
        assert!(p.next("a").is_ok());
        assert!(p.next("b").is_ok());
        assert!(p.next("a").is_err());
    }
}
