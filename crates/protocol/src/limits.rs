//! Request limits shared by the server and the browser. Exported to `generated/limits.ts`.

macro_rules! limits {
    (
        numbers { $( $(#[doc = $ndoc:literal])* $nname:ident: $nty:ty = $nvalue:expr; )* }
        strings { $( $(#[doc = $sdoc:literal])* $sname:ident = $svalue:literal; )* }
    ) => {
        $( $(#[doc = $ndoc])* pub const $nname: $nty = $nvalue; )*
        $( $(#[doc = $sdoc])* pub const $sname: &str = $svalue; )*

        /// Every numeric limit with its doc text, for export.
        pub const NUMBERS: &[(&str, &str, i64)] = &[
            $( (stringify!($nname), concat!($($ndoc),*), $nname as i64), )*
        ];

        /// Every string constant with its doc text, for export.
        pub const STRINGS: &[(&str, &str, &str)] = &[
            $( (stringify!($sname), concat!($($sdoc),*), $sname), )*
        ];
    };
}

limits! {
    numbers {
        /// Longest accepted idempotency key, in characters.
        IDEMPOTENCY_KEY_MAX_CHARS: usize = 128;
        /// How long a stored command answer can be replayed, in milliseconds.
        IDEMPOTENCY_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
        /// Largest command body, in bytes.
        COMMAND_BODY_MAX_BYTES: usize = 64 * 1024;
        /// Longest query string, in bytes. Matches the 8 KiB exploration URL limit of decision 0004.
        QUERY_STRING_MAX_BYTES: usize = 8 * 1024;
        /// Server-side budget for one API request, in milliseconds.
        REQUEST_TIMEOUT_MS: u64 = 30_000;
        /// Default page size for paged queries.
        PAGE_SIZE_DEFAULT: u32 = 25;
        /// Largest page size for paged queries.
        PAGE_SIZE_MAX: u32 = 100;
        /// Formula version this server computes (decision 0003). States pinned to another are refused.
        METRIC_VERSION: u32 = 1;
        /// Measures per exploration (decision 0004).
        EXPLORATION_MEASURES_MAX: usize = 8;
        /// Dimensions per exploration.
        EXPLORATION_DIMENSIONS_MAX: usize = 3;
        /// Sort keys per exploration.
        EXPLORATION_SORT_MAX: usize = 2;
        /// Conditions per exploration state, counted across every group.
        EXPLORATION_CONDITIONS_MAX: usize = 32;
        /// Group nesting depth: the root group plus two nested levels.
        EXPLORATION_FILTER_DEPTH_MAX: usize = 3;
        /// Entries per group.
        EXPLORATION_GROUP_ITEMS_MAX: usize = 32;
        /// Values in one `in` or `not_in` list.
        EXPLORATION_LIST_ITEMS_MAX: usize = 50;
        /// Longest property key, in bytes.
        EXPLORATION_PROPERTY_KEY_MAX_BYTES: usize = 500;
        /// Default rows per exploration page.
        EXPLORATION_PAGE_SIZE_DEFAULT: u32 = 50;
        /// Rows a query can address: page index times size.
        EXPLORATION_ROWS_MAX: u32 = 10_000;
        /// Points in one series, 31 days hourly.
        EXPLORATION_SERIES_POINTS_MAX: usize = 744;
        /// Longest relative range, in days.
        EXPLORATION_RELATIVE_DAYS_MAX: u32 = 366;
        /// Extra panels computed with one exploration.
        EXPLORATION_PANELS_MAX: usize = 8;
        /// Weeks a typical-weekday baseline reaches back: days `d − 7k` for `k = 1..8` (decision 0015).
        TYPICAL_WEEKDAY_WEEKS: u32 = 8;
        /// Fewest baseline days a typical-weekday baseline or projection needs; fewer answer `insufficient_history`.
        TYPICAL_WEEKDAY_DAYS_MIN: u32 = 4;
        /// Longest range a typical-weekday comparison accepts, in days; longer ones are refused with `compare_unsupported`.
        TYPICAL_WEEKDAY_RANGE_DAYS_MAX: u32 = 31;
        /// Counted units a baseline day needs before now for a projection to scale it, for money its revenue records of the currency; below this the scale is 1.
        PROJECTION_SCALING_FLOOR: i64 = 20;
        /// Sites one `summaries.forSites` call names.
        SUMMARY_SITES_MAX: usize = 100;
        /// Shortest time between two summaries of a site after its watermark moved, in milliseconds (decision 0015).
        SUMMARY_CADENCE_MS: i64 = 10 * 60 * 1000;
        /// Age after which a site summary is stale and the UI drops its pace, in milliseconds.
        SUMMARY_STALE_MS: i64 = 30 * 60 * 1000;
        /// Findings kept per site, in rank order; also the largest `summaries.findings` limit.
        FINDINGS_MAX: usize = 20;
        /// Shortest time between two runs of the performance and funnel step rules for a site, in milliseconds (decision 0015 fallback cadence).
        FINDINGS_HOURLY_CADENCE_MS: i64 = 60 * 60 * 1000;
        /// A dimension mover changes by at least this many visitor-days against a typical weekday so far.
        FINDING_MOVER_CHANGE_MIN: i64 = 20;
        /// A rising dimension mover reaches at least this percentage of its typical value: 150 is 1.5 times.
        FINDING_MOVER_RISE_PERCENT: i64 = 150;
        /// A falling dimension mover reaches at most this percentage of its typical value: 67 is 0.67 times.
        FINDING_MOVER_FALL_PERCENT: i64 = 67;
        /// A dimension mover's change is at least this percentage of the site's total change, in the same direction.
        FINDING_MOVER_SHARE_PERCENT: i64 = 25;
        /// A goal's completions so far differ from its typical weekday at this hour by at least this percentage.
        FINDING_GOAL_CHANGE_PERCENT: i64 = 30;
        /// A goal's completions so far differ from its typical weekday at this hour by at least this many.
        FINDING_GOAL_CHANGE_MIN: i64 = 5;
        /// Active goals the goal pace rule reads per site, oldest first.
        FINDING_GOALS_MAX: usize = 8;
        /// A funnel step's drop-off changes by at least this many percentage points against the typical weekday.
        FINDING_FUNNEL_POINTS_MIN: i64 = 5;
        /// Daily visitors entering a funnel step, today and over the baseline days, for its drop-off to count.
        FINDING_FUNNEL_ENTERED_MIN: i64 = 50;
        /// Funnels on boards the funnel step rule reads per site.
        FINDING_FUNNELS_MAX: usize = 8;
        /// Observations a metric and device need in the last 24 hours for the performance rule.
        FINDING_PERFORMANCE_OBSERVATIONS_MIN: i64 = 100;
        /// Days before the last 24 hours whose daily p75 the performance rule compares with.
        FINDING_PERFORMANCE_DAYS: u32 = 28;
        /// Of those, days with observations the performance rule needs before it compares.
        FINDING_PERFORMANCE_DAYS_MIN: u32 = 4;
        /// Fewest funnel steps (decision 0004 Report definitions).
        REPORT_FUNNEL_STEPS_MIN: usize = 2;
        /// Most funnel steps.
        REPORT_FUNNEL_STEPS_MAX: usize = 8;
        /// Shortest funnel window, in seconds.
        REPORT_FUNNEL_WINDOW_MIN_SECONDS: u32 = 60;
        /// Longest funnel window, in seconds: 30 days.
        REPORT_FUNNEL_WINDOW_MAX_SECONDS: u32 = 30 * 86_400;
        /// Shortest journey length, in steps.
        REPORT_JOURNEY_LENGTH_MIN: u8 = 2;
        /// Longest journey length, in steps.
        REPORT_JOURNEY_LENGTH_MAX: u8 = 7;
        /// Journey sequences in a result, by session count.
        REPORT_JOURNEY_SEQUENCES_MAX: usize = 100;
        /// Conversions per attribution report.
        REPORT_ATTRIBUTION_CONVERSIONS_MAX: usize = 4;
        /// Touch values per attribution conversion in a result.
        REPORT_ATTRIBUTION_TOUCHES_MAX: usize = 100;
        /// The attribution lookback, in days. Fixed in version 1.
        REPORT_ATTRIBUTION_LOOKBACK_DAYS: u16 = 30;
        /// Most retention intervals for day cohorts.
        REPORT_RETENTION_DAY_INTERVALS_MAX: u8 = 32;
        /// Most retention intervals for week cohorts.
        REPORT_RETENTION_WEEK_INTERVALS_MAX: u8 = 53;
        /// Cohort periods in range times intervals.
        REPORT_RETENTION_CELLS_MAX: usize = 3_000;
        /// Columns of a session list.
        REPORT_SESSION_LIST_COLUMNS_MAX: usize = 16;
        /// Longest session timeline window, in hours.
        SESSION_TIMELINE_HOURS_MAX: u32 = 24;
        /// Sessions on one timeline, the latest starts.
        SESSION_TIMELINE_SESSIONS_MAX: usize = 300;
        /// Events per session on a timeline.
        SESSION_TIMELINE_EVENTS_MAX: usize = 60;
        /// Goals per goals report.
        REPORT_GOALS_MAX: usize = 8;
        /// Longest name of a saved exploration, segment or board, in characters after trimming.
        DEFINITION_NAME_MAX_CHARS: usize = 100;
        /// Longest annotation text, in characters after trimming.
        ANNOTATION_TEXT_MAX_CHARS: usize = 500;
        /// Cards on one board.
        BOARD_CARDS_MAX: usize = 24;
        /// Longest site name, in characters, after trimming.
        SITE_NAME_MAX_CHARS: usize = 100;
        /// Longest site domain, in characters.
        SITE_DOMAIN_MAX_CHARS: usize = 253;
        /// Longest slug of a link or pixel, in characters.
        SLUG_MAX_CHARS: usize = 100;
        /// Characters of a slug the server picks when none is given.
        SLUG_GENERATED_CHARS: usize = 8;
        /// Longest link target URL, in characters.
        LINK_URL_MAX_CHARS: usize = 2048;
        /// Most identity days one recalculation may cover: the retained 12 months.
        RECALCULATION_MAX_DAYS: i64 = 366;
        /// Login attempts per client address per minute.
        LOGIN_ATTEMPTS_PER_MINUTE: u32 = 10;
        /// Shortest password, in characters.
        PASSWORD_MIN_CHARS: usize = 12;
        /// Longest password, in characters. Bounds the work of one hash.
        PASSWORD_MAX_CHARS: usize = 256;
        /// Longest person or team name, in characters after trimming.
        ACCOUNT_NAME_MAX_CHARS: usize = 100;
        /// Longest email address, in characters.
        EMAIL_MAX_CHARS: usize = 254;
        /// A login session ends this long after sign-in, in milliseconds (decision 0012).
        SESSION_LIFETIME_MS: i64 = 30 * 24 * 60 * 60 * 1000;
        /// A login session unused for this long ends, in milliseconds (decision 0012).
        SESSION_IDLE_MS: i64 = 7 * 24 * 60 * 60 * 1000;
        /// A password check marks the login session for this long, in milliseconds (decision 0007).
        REAUTHENTICATION_WINDOW_MS: i64 = 10 * 60 * 1000;
        /// The second-factor step after a correct password must finish within this, in milliseconds.
        LOGIN_CHALLENGE_MS: i64 = 5 * 60 * 1000;
        /// Wrong codes one login challenge accepts before it ends.
        LOGIN_CHALLENGE_ATTEMPTS: u32 = 5;
        /// Digits of a time-based one-time code (RFC 6238, decision 0012).
        TOTP_DIGITS: u32 = 6;
        /// Seconds one time-based code is valid for.
        TOTP_PERIOD_SECONDS: u32 = 30;
        /// Recovery codes issued at enrollment and on regeneration. Each works once.
        RECOVERY_CODES: usize = 10;
        /// Active API keys per person.
        API_KEYS_PER_USER_MAX: usize = 50;
        /// Active collection keys per site.
        COLLECTION_KEYS_PER_SITE_MAX: usize = 20;
        /// Longest API key, collection key, share or invitation label, in characters after trimming.
        LABEL_MAX_CHARS: usize = 100;
        /// Longest invitation lifetime, in days.
        INVITATION_LIFETIME_MAX_DAYS: u32 = 30;
        /// Longest share lifetime, in days. Every share expires.
        SHARE_LIFETIME_MAX_DAYS: u32 = 366;
    }
    strings {
        /// Header every API call sends. Commands without it are refused with `origin_rejected`.
        REQUEST_MARKER_HEADER = "x-holocron-request";
        /// Value of the request marker header.
        REQUEST_MARKER_VALUE = "1";
        /// `Authorization` scheme of API keys: `Bearer <secret>`. Commands sent with a key skip
        /// the request marker and Origin checks; browsers cannot attach the header cross-site.
        API_KEY_SCHEME = "Bearer";
        /// `Authorization` scheme of site collection keys on `/api/send` and `/api/batch`:
        /// `Bearer <secret>`. Only server integrations send it.
        COLLECTION_KEY_SCHEME = "Bearer";
        /// `Authorization` scheme of share viewers: `Share <token>`. Unknown, revoked and
        /// expired tokens answer `not_found` for every operation.
        SHARE_SCHEME = "Share";
    }
}
