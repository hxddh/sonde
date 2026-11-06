//! Concurrency and request-rate limits shared by the requests of a job.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// A limit on requests in flight and on requests per second.
#[derive(Debug)]
pub struct Throttle {
    permits: Arc<Semaphore>,
    concurrency: usize,
    rate: Option<f64>,
    bucket: Mutex<(f64, Instant)>,
}

impl Throttle {
    /// `concurrency` requests at once; `rate` per second at most (`None`: no limit).
    pub fn new(concurrency: usize, rate: Option<f64>) -> Self {
        let concurrency = concurrency.max(1);
        let rate = rate.filter(|r| *r > 0.0);
        Throttle {
            permits: Arc::new(Semaphore::new(concurrency)),
            concurrency,
            rate,
            bucket: Mutex::new((rate.unwrap_or(1.0).max(1.0), Instant::now())),
        }
    }

    pub fn concurrency(&self) -> usize {
        self.concurrency
    }

    pub fn rate(&self) -> Option<f64> {
        self.rate
    }

    /// Waits for a slot and a token. Dropping the permit frees the slot.
    pub async fn acquire(&self) -> OwnedSemaphorePermit {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore never closed");
        if let Some(rate) = self.rate {
            loop {
                let wait = {
                    let mut b = self.bucket.lock();
                    let now = Instant::now();
                    let burst = rate.max(1.0);
                    b.0 = (b.0 + now.duration_since(b.1).as_secs_f64() * rate).min(burst);
                    b.1 = now;
                    if b.0 >= 1.0 {
                        b.0 -= 1.0;
                        None
                    } else {
                        Some(Duration::from_secs_f64((1.0 - b.0) / rate))
                    }
                };
                match wait {
                    None => break,
                    Some(d) => tokio::time::sleep(d).await,
                }
            }
        }
        permit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limits() {
        let t = Throttle::new(8, Some(20.0));
        let start = Instant::now();
        for _ in 0..30 {
            drop(t.acquire().await);
        }
        // 20 burst + 10 at 20/s ≈ 0.5 s
        let e = start.elapsed();
        assert!(e >= Duration::from_millis(400), "{e:?}");
    }

    #[tokio::test]
    async fn concurrency_limits() {
        let t = Arc::new(Throttle::new(2, None));
        let a = t.acquire().await;
        let _b = t.acquire().await;
        let t2 = t.clone();
        let waiter = tokio::spawn(async move { drop(t2.acquire().await) });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter.is_finished());
        drop(a);
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
    }
}
