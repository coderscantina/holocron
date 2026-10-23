//! Opaque identifiers. They travel as strings and are compared byte for byte.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
        )]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// A person who signs in.
    UserId
);
id_type!(
    /// A personal or team workspace that owns sites.
    WorkspaceId
);
id_type!(
    /// A tracked website. Matches the `website` ID the Umami tracker sends.
    SiteId
);
id_type!(
    /// One requested recalculation of a site's derived data.
    RecalculationId
);
id_type!(
    /// A saved exploration: a named, complete exploration state of one person.
    ExplorationId
);
id_type!(
    /// A named, reusable filter group of one site.
    SegmentId
);
id_type!(
    /// A saved, versioned goal definition of one site.
    GoalId
);
id_type!(
    /// A note pinned to an instant on a site's charts.
    AnnotationId
);
id_type!(
    /// A workspace board of exploration cards.
    BoardId
);
id_type!(
    /// One card of a board.
    CardId
);
id_type!(
    /// A scoped API key. Not the secret: the secret is shown once and stored only as a hash.
    ApiKeyId
);
id_type!(
    /// A site collection key for trusted server integrations. Not the secret: the secret is
    /// shown once and stored only as a hash.
    CollectionKeyId
);
id_type!(
    /// A restricted share of an exploration or a board. Not the viewer token.
    ShareId
);
id_type!(
    /// A team invitation. Not the invitation code.
    InvitationId
);
id_type!(
    /// A tracked link: `/q/<slug>` counts a hit and redirects to its URL.
    LinkId
);
id_type!(
    /// A tracking pixel: `/p/<slug>` counts a hit and answers a 1x1 image.
    PixelId
);
