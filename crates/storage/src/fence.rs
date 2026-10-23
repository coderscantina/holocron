//! Erasure fences: once a fence commits, no writer may put the records it covers into
//! analytics again. Transfer checks every record of a batch, imported ones included; an
//! unfinished import refuses erasure jobs, so the two never overlap (decision 0014).

use std::collections::HashMap;

use crate::record::{Id, Record};

/// What an erasure removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Erasure {
    /// Site deletion: every record of the site, admitted before or after the fence.
    Site(Id),
    /// Site reset: the site's records admitted before the fence. Later records are new data.
    SiteHistory(Id),
    /// One persistent profile's records admitted before the fence.
    Profile { site: Id, profile: Id },
}

impl Erasure {
    pub fn site(&self) -> Id {
        match *self {
            Erasure::Site(site) | Erasure::SiteHistory(site) => site,
            Erasure::Profile { site, .. } => site,
        }
    }
}

/// A committed fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fence {
    pub erasure: Erasure,
    /// The last inbox sequence admitted when the fence committed.
    pub through_seq: i64,
    pub created_at_ms: i64,
}

impl Fence {
    /// Whether this fence covers a record of `site` linked to `profile`, admitted at `seq`.
    pub fn blocks(&self, site: Id, profile: Option<Id>, seq: i64) -> bool {
        let before = seq <= self.through_seq;
        match self.erasure {
            Erasure::Site(fenced) => fenced == site,
            Erasure::SiteHistory(fenced) => fenced == site && before,
            Erasure::Profile {
                site: fenced,
                profile: fenced_profile,
            } => fenced == site && profile == Some(fenced_profile) && before,
        }
    }
}

/// Every committed fence, by site.
#[derive(Debug, Clone, Default)]
pub struct Fences(HashMap<Id, Vec<Fence>>);

impl Fences {
    pub(crate) fn insert(&mut self, fence: Fence) {
        let fences = self.0.entry(fence.erasure.site()).or_default();
        if !fences.contains(&fence) {
            fences.push(fence);
        }
    }

    /// Whether transfer must leave out an inbox record.
    pub(crate) fn blocks_record(&self, seq: i64, record: &Record) -> bool {
        let profile = match record {
            Record::Event(e) => e.profile_id,
            Record::Identify(i) => i.profile_id,
            Record::Revocation(r) => r.profile_id,
            Record::ReplayFragment(_) | Record::HeatmapInteraction(_) => None,
        };
        let site = record.site();
        self.0
            .get(&site)
            .is_some_and(|fences| fences.iter().any(|f| f.blocks(site, profile, seq)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: Id = 7;
    const OTHER: Id = 8;

    fn fence(erasure: Erasure) -> Fence {
        Fence {
            erasure,
            through_seq: 100,
            created_at_ms: 5_000,
        }
    }

    #[test]
    fn site_deletion_covers_every_record_of_the_site() {
        let f = fence(Erasure::Site(SITE));
        assert!(f.blocks(SITE, None, 1_000));
        assert!(!f.blocks(OTHER, None, 1));
    }

    #[test]
    fn reset_and_profile_fences_cover_only_earlier_records() {
        let reset = fence(Erasure::SiteHistory(SITE));
        assert!(reset.blocks(SITE, None, 100));
        assert!(!reset.blocks(SITE, None, 101));

        let profile = fence(Erasure::Profile {
            site: SITE,
            profile: 3,
        });
        assert!(profile.blocks(SITE, Some(3), 50));
        assert!(!profile.blocks(SITE, Some(4), 50));
        assert!(!profile.blocks(SITE, None, 50));
        assert!(!profile.blocks(SITE, Some(3), 101));
    }
}
