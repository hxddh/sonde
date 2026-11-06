//! sonde's compatibility probes.
//!
//! A [`Probe`] states what AWS S3 does ([`ProbeMeta::aws_behavior`]),
//! exercises it against an endpoint in a scratch prefix (and, when allowed,
//! temporary buckets), records each request as a [`model::Step`], and
//! judges the steps with a pure function ([`Probe::judge`]) — so the
//! judgement is unit-tested on recorded responses (`fixtures/`). The
//! runner ([`runner`]) cleans up after every probe, keeps the raw
//! exchanges, saves runs by time ([`store`]) and compares them.

pub mod cx;
pub mod model;
pub mod report;
pub mod runner;
pub mod store;
pub mod suite;

use async_trait::async_trait;

pub use cx::ProbeCx;
pub use model::{Category, Judgement, ProbeMeta, ProbeResult, Run, Step, Steps, Verdict};

#[async_trait]
pub trait Probe: Send + Sync {
    fn meta(&self) -> ProbeMeta;
    /// Makes the requests, recording each as a step. Never fails: a failed
    /// request is a step the judge reads.
    async fn run(&self, cx: &ProbeCx);
    /// Judges recorded steps. Pure, so fixtures test it.
    fn judge(&self, steps: &Steps) -> Judgement;
}

/// The metadata of every probe.
pub fn catalog() -> Vec<ProbeMeta> {
    suite::all().iter().map(|p| p.meta()).collect()
}

/// A probe by id.
pub fn probe(id: &str) -> Option<Box<dyn Probe>> {
    suite::all().into_iter().find(|p| p.meta().id == id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_unique_and_documented() {
        let c = super::catalog();
        let mut ids: Vec<&str> = c.iter().map(|m| m.id.as_str()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(n >= 40, "{n} probes");
        for m in &c {
            assert!(
                !m.aws_behavior.is_empty() && !m.description.is_empty(),
                "{}",
                m.id
            );
        }
    }
}
