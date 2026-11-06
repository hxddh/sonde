//! The probes, by category.

pub mod checksums;
pub mod conditional;
pub mod consistency;
pub mod listing;
pub mod misc;
pub mod multipart;
pub mod range;

use crate::Probe;
use crate::model::{Category, Needs, ProbeMeta};

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
