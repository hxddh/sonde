//! The probes, by category.

pub mod checksums;
pub mod conditional;
pub mod consistency;
pub mod listing;
pub mod misc;
pub mod multipart;
pub mod range;

use crate::Probe;
use crate::model::{Category, Judgement, Needs, ProbeMeta, Steps, Verdict};

/// `Error` when a step the probe only prepares with (`labels`: the object
/// it then tests on) did not succeed: the probe could not run, which is
/// neither a pass nor a broken guarantee. `None` when they all did, or
/// when a step says "not implemented" (the judge says unsupported then).
pub(crate) fn setup_failed(s: &Steps, labels: &[&str]) -> Option<Judgement> {
    if s.any_not_implemented().is_some() {
        return None;
    }
    let failed = labels.iter().map(|l| s.at(l)).find(|x| !x.ok())?;
    Some(Judgement::new(
        Verdict::Error,
        format!("could not run: setup failed ({})", failed.describe()),
        s,
    ))
}

/// `Error` when a write that is itself the test (`labels`) was not let
/// through: no answer at all, or 403 AccessDenied (a bucket that may not be
/// written). That says nothing of the feature, where a 400 or a 501 does.
pub(crate) fn write_refused(s: &Steps, labels: &[&str]) -> Option<Judgement> {
    // a step that never ran was not refused
    let failed = labels.iter().filter_map(|l| s.get(l)).find(|x| {
        x.status.is_none()
            || (x.is(403)
                && matches!(
                    x.code.as_deref(),
                    Some("AccessDenied" | "AllAccessDisabled" | "InvalidAccessKeyId")
                ))
    })?;
    Some(Judgement::new(
        Verdict::Error,
        format!(
            "could not run: the write was not allowed ({})",
            failed.describe()
        ),
        s,
    ))
}

pub(crate) fn meta(
    id: &str,
    category: Category,
    title: &str,
    description: &str,
    aws: &str,
    needs: &[Needs],
) -> ProbeMeta {
    ProbeMeta {
        id: id.into(),
        category,
        title: title.into(),
        description: description.into(),
        aws_behavior: aws.into(),
        needs: needs.to_vec(),
    }
}

/// Every probe, in display order.
pub fn all() -> Vec<Box<dyn Probe>> {
    let mut v: Vec<Box<dyn Probe>> = Vec::new();
    v.extend(conditional::probes());
    v.extend(consistency::probes());
    v.extend(checksums::probes());
    v.extend(multipart::probes());
    v.extend(range::probes());
    v.extend(listing::probes());
    v.extend(misc::probes());
    v
}
