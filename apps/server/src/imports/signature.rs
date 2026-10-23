//! The two supported Umami layouts and the refusal rules of `fixtures/import-schemas`.
//!
//! A source is accepted only when its `_prisma_migrations` table and its columns match one of
//! the captured snapshots. Column types match exactly, except that MySQL may use another type
//! of the same family (see [`family`]); those columns are accepted and reported. The snapshots
//! are embedded from the fixtures, so the inspection and the fixture tests read the same files.
//! Checks run in the order the fixture README gives: engine and pairing, failed migrations,
//! missing, unknown and modified migrations, then tables and columns. Every message names what
//! failed and says the source was not changed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Value, json};

/// The source database engine, from the connection URL scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Postgresql,
    Mysql,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Postgresql => "postgresql",
            Engine::Mysql => "mysql",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Engine::Postgresql => "PostgreSQL",
            Engine::Mysql => "MySQL",
        }
    }
}

/// An accepted source layout. Each has its own tables, so the importer reads by layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Umami 3.4.0 on PostgreSQL, 26 migrations.
    Umami3Postgresql,
    /// Umami 2.20.2 on MySQL, 13 migrations.
    Umami2Mysql,
}

impl Layout {
    pub fn engine(self) -> Engine {
        match self {
            Layout::Umami3Postgresql => Engine::Postgresql,
            Layout::Umami2Mysql => Engine::Mysql,
        }
    }

    pub fn version(self) -> &'static str {
        match self {
            Layout::Umami3Postgresql => "3.4.0",
            Layout::Umami2Mysql => "2.20.2",
        }
    }

    /// `umami-<version>-<engine>`: names the source in manifests and imported history.
    pub fn source_name(self) -> String {
        format!("umami-{}-{}", self.version(), self.engine().name())
    }

    fn of(engine: Engine) -> Self {
        match engine {
            Engine::Postgresql => Layout::Umami3Postgresql,
            Engine::Mysql => Layout::Umami2Mysql,
        }
    }

    fn snapshot(self) -> &'static Snapshot {
        static PG: OnceLock<Snapshot> = OnceLock::new();
        static MY: OnceLock<Snapshot> = OnceLock::new();
        match self {
            Layout::Umami3Postgresql => PG.get_or_init(|| {
                Snapshot::parse(
                    include_str!(
                        "../../../../fixtures/import-schemas/postgresql-3.4.0/migrations.json"
                    ),
                    include_str!(
                        "../../../../fixtures/import-schemas/postgresql-3.4.0/columns.json"
                    ),
                )
            }),
            Layout::Umami2Mysql => MY.get_or_init(|| {
                Snapshot::parse(
                    include_str!(
                        "../../../../fixtures/import-schemas/mysql-2.20.2/migrations.json"
                    ),
                    include_str!("../../../../fixtures/import-schemas/mysql-2.20.2/columns.json"),
                )
            }),
        }
    }
}

/// One `_prisma_migrations` row as the source reports it.
#[derive(Debug, Clone)]
pub struct Migration {
    pub name: String,
    pub checksum: String,
    pub finished: bool,
    pub rolled_back: bool,
}

/// One column as the source catalog reports it.
#[derive(Debug, Clone)]
pub struct Column {
    pub table: String,
    pub column: String,
    pub data_type: String,
    pub nullable: bool,
}

/// What inspection read from a source.
#[derive(Debug, Clone)]
pub struct Observed {
    pub engine: Engine,
    pub migrations: Vec<Migration>,
    pub columns: Vec<Column>,
}

/// An actionable refusal: a stable code, the failing names, and operator guidance.
#[derive(Debug, Clone)]
pub struct Refusal {
    pub code: &'static str,
    pub details: Value,
    pub message: String,
}

pub enum Verdict {
    Accept(Accepted),
    Refuse(Refusal),
}

/// An accepted source and the columns whose type differs within its family.
#[derive(Debug, Clone)]
pub struct Accepted {
    pub layout: Layout,
    /// `{table, column, expected, found}` per tolerated column.
    pub tolerated: Vec<Value>,
}

impl Accepted {
    /// The inspection's account of the tolerated columns, `None` when the types match exactly.
    pub fn report(&self) -> Option<Value> {
        if self.tolerated.is_empty() {
            return None;
        }
        let datetime = self.tolerated.iter().any(|c| {
            c["found"]
                .as_str()
                .is_some_and(|t| t.starts_with("datetime"))
        });
        let mut message = format!(
            "{} column{} differ{} from Umami {} in a way the import reads the same. Text longer than Umami {} allows is cut to its length.",
            self.tolerated.len(),
            plural(self.tolerated.len()),
            if self.tolerated.len() == 1 { "s" } else { "" },
            self.layout.version(),
            self.layout.version()
        );
        if datetime {
            message.push_str(
                " Values of datetime columns carry no timezone and are read as UTC, the way Umami writes them.",
            );
        }
        Some(json!({ "columns": self.tolerated, "message": message }))
    }
}

struct Snapshot {
    /// Name and checksum, sorted by name, which is migration order.
    migrations: Vec<(String, String)>,
    /// Table name to column name to (type, nullable).
    tables: BTreeMap<String, BTreeMap<String, (String, bool)>>,
}

impl Snapshot {
    fn parse(migrations: &str, columns: &str) -> Self {
        #[derive(Deserialize)]
        struct Row {
            migration_name: String,
            checksum: String,
        }
        #[derive(Deserialize)]
        struct Inventory {
            tables: Vec<Table>,
        }
        #[derive(Deserialize)]
        struct Table {
            name: String,
            columns: Vec<TableColumn>,
        }
        #[derive(Deserialize)]
        struct TableColumn {
            name: String,
            #[serde(rename = "type")]
            data_type: String,
            nullable: bool,
        }
        let rows: Vec<Row> = serde_json::from_str(migrations).expect("embedded migrations.json");
        let inventory: Inventory = serde_json::from_str(columns).expect("embedded columns.json");
        Self {
            migrations: rows
                .into_iter()
                .map(|r| (r.migration_name, r.checksum))
                .collect(),
            tables: inventory
                .tables
                .into_iter()
                .map(|t| {
                    let columns = t
                        .columns
                        .into_iter()
                        .map(|c| (c.name, (c.data_type, c.nullable)))
                        .collect();
                    (t.name, columns)
                })
                .collect(),
        }
    }

    fn names(&self) -> BTreeSet<&str> {
        self.migrations.iter().map(|(n, _)| n.as_str()).collect()
    }
}

/// The 13 migrations Umami 2.20.2 on PostgreSQL shares with 3.4.0 (`01_` to `13_`).
const PG_SHARED_WITH_V2: usize = 13;

const UNCHANGED: &str = "Holocron did not change the source.";

/// Accepts a source matching a supported snapshot, or names the first rule it breaks.
pub fn check(observed: &Observed) -> Verdict {
    let engine = observed.engine;
    let layout = Layout::of(engine);
    let expected = layout.snapshot();
    let applied: BTreeSet<&str> = observed
        .migrations
        .iter()
        .map(|m| m.name.as_str())
        .collect();

    if engine == Engine::Mysql {
        let v3_only: BTreeSet<&str> = Layout::Umami3Postgresql
            .snapshot()
            .names()
            .difference(&expected.names())
            .copied()
            .collect();
        let claimed: Vec<&str> = applied.intersection(&v3_only).copied().collect();
        if !claimed.is_empty() {
            return refuse(
                "unsupported_engine",
                json!({ "unknown": claimed }),
                format!(
                    "This MySQL database lists an Umami 3.x migration ({}). Umami 3.x supports only PostgreSQL, so this database is not a supported layout. Holocron imports MySQL only at Umami 2.20.2 and PostgreSQL only at 3.4.0. Restore the MySQL database at 2.20.2, or migrate the data to PostgreSQL with Umami 3.4.0, then run the inspection again.",
                    claimed.join(", ")
                ),
            );
        }
    }

    if let Some(failed) = observed
        .migrations
        .iter()
        .find(|m| !m.finished || m.rolled_back)
    {
        let names: Vec<&str> = observed
            .migrations
            .iter()
            .filter(|m| !m.finished || m.rolled_back)
            .map(|m| m.name.as_str())
            .collect();
        let message = if failed.rolled_back {
            format!(
                "Migration {} is marked rolled back and never finished. The source is not at the Umami {} schema. Re-apply it with Umami {} (prisma migrate deploy) and run the inspection again.",
                failed.name,
                layout.version(),
                layout.version()
            )
        } else {
            format!(
                "Migration {} started but never finished (finished_at is empty). The source is partially migrated. Read _prisma_migrations.logs for the cause, repair the source with Umami {} (prisma migrate resolve, then prisma migrate deploy), and run the inspection again.",
                failed.name,
                layout.version()
            )
        };
        return refuse(
            "migration_failed",
            json!({
                "failed": names,
                "finishedAt": failed.finished.then_some("set"),
                "rolledBackAt": failed.rolled_back.then_some("set"),
            }),
            message,
        );
    }

    let missing: Vec<&str> = expected
        .migrations
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| !applied.contains(n))
        .collect();

    if engine == Engine::Postgresql {
        let shared: BTreeSet<&str> = expected.migrations[..PG_SHARED_WITH_V2]
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        if applied == shared {
            return refuse(
                "unsupported_pairing",
                json!({ "applied": applied.len(), "missing": missing }),
                format!(
                    "This PostgreSQL database has exactly the {PG_SHARED_WITH_V2} migrations of Umami 2.x. Holocron imports Umami 2.x only from MySQL 2.20.2, and PostgreSQL only at 3.4.0. Upgrade the source to Umami 3.4.0 (it applies migrations {} to {}), then run the inspection again. If this database is a stopped 3.x upgrade, finish the upgrade the same way.",
                    prefix(missing.first()),
                    prefix(missing.last())
                ),
            );
        }
    }

    if !missing.is_empty() {
        let message = match engine {
            Engine::Postgresql => format!(
                "This PostgreSQL database is missing the Umami migration{} {}. Holocron imports only the fully migrated Umami 3.4.0 schema ({} migrations). Upgrade the source to Umami 3.4.0 and let it finish `prisma migrate deploy`, or restore a backup taken after the upgrade, then run the inspection again.",
                plural(missing.len()),
                missing.join(", "),
                expected.migrations.len()
            ),
            Engine::Mysql => format!(
                "This MySQL database is missing the Umami migration{} {}. Upgrade the source to Umami 2.20.2 (the last release with MySQL support) so it applies all {} migrations, then run the inspection again. Do not upgrade to Umami 3.x, it does not support MySQL.",
                plural(missing.len()),
                missing.join(", "),
                expected.migrations.len()
            ),
        };
        return refuse("migration_missing", json!({ "missing": missing }), message);
    }

    let known = expected.names();
    let unknown: Vec<&str> = applied.difference(&known).copied().collect();
    if !unknown.is_empty() {
        return refuse(
            "migration_unknown",
            json!({ "unknown": unknown }),
            format!(
                "This {} database has a migration Holocron does not know: {}. The source is newer than Umami {} or was modified. Holocron imports only the exact {} schema. Restore a snapshot taken at {}, or export from the source with its own tools, then run the inspection again.",
                engine.label(),
                unknown.join(", "),
                layout.version(),
                layout.version(),
                layout.version()
            ),
        );
    }

    let checksums: BTreeMap<&str, &str> = observed
        .migrations
        .iter()
        .map(|m| (m.name.as_str(), m.checksum.as_str()))
        .collect();
    let modified: Vec<&str> = expected
        .migrations
        .iter()
        .filter(|(name, checksum)| checksums.get(name.as_str()) != Some(&checksum.as_str()))
        .map(|(name, _)| name.as_str())
        .collect();
    if !modified.is_empty() {
        return refuse(
            "migration_modified",
            json!({ "modified": modified }),
            format!(
                "The recorded checksum of {} differs from Umami {}. The migration files were edited before they ran, so the schema may differ. Restore a backup of an unmodified Umami {} installation and run the inspection again.",
                modified.join(", "),
                layout.version(),
                layout.version()
            ),
        );
    }

    let mut found: BTreeMap<&str, BTreeMap<&str, (&str, bool)>> = BTreeMap::new();
    for c in &observed.columns {
        found
            .entry(c.table.as_str())
            .or_default()
            .insert(c.column.as_str(), (c.data_type.as_str(), c.nullable));
    }
    let tables_missing: Vec<&str> = expected
        .tables
        .keys()
        .map(String::as_str)
        .filter(|t| !found.contains_key(t))
        .collect();
    if !tables_missing.is_empty() {
        return refuse(
            "table_missing",
            json!({ "migrationTableValid": true, "missing": tables_missing }),
            format!(
                "Table{} {} {} missing, although all {} migrations are recorded. The schema was changed by hand after migration. Restore a backup of the source and run the inspection again.",
                plural(tables_missing.len()),
                tables_missing.join(", "),
                if tables_missing.len() == 1 {
                    "is"
                } else {
                    "are"
                },
                expected.migrations.len()
            ),
        );
    }
    let mut columns_missing = Vec::new();
    let mut changed = Vec::new();
    let mut tolerated = Vec::new();
    for (table, columns) in &expected.tables {
        let have = &found[table.as_str()];
        for (column, (data_type, nullable)) in columns {
            match have.get(column.as_str()) {
                None => columns_missing.push(json!({ "table": table, "column": column })),
                Some((t, n)) if *t == data_type && *n == *nullable => {}
                Some((t, n))
                    if engine == Engine::Mysql
                        && *n == *nullable
                        && family(data_type).is_some_and(|f| family(t) == Some(f)) =>
                {
                    tolerated.push(json!({
                        "table": table, "column": column, "expected": data_type, "found": t,
                    }))
                }
                Some((t, n)) => changed.push(json!({
                    "table": table, "column": column,
                    "expected": { "type": data_type, "nullable": nullable },
                    "found": { "type": t, "nullable": n },
                })),
            }
        }
    }
    if let Some(first) = columns_missing.first() {
        let guidance = match engine {
            Engine::Postgresql => {
                "Rename or re-add the column as Umami defines it (or restore a backup)"
            }
            Engine::Mysql => {
                "Restore a backup of the source, or re-add the column as Umami defines it"
            }
        };
        let message = format!(
            "Table {} has no column {}{}, although all {} migrations are recorded. The schema was changed by hand after migration. {guidance}, and run the inspection again.",
            first["table"].as_str().unwrap_or_default(),
            first["column"].as_str().unwrap_or_default(),
            if columns_missing.len() > 1 {
                format!(" (and {} more)", columns_missing.len() - 1)
            } else {
                String::new()
            },
            expected.migrations.len()
        );
        return refuse(
            "column_missing",
            json!({ "migrationTableValid": true, "missing": columns_missing }),
            message,
        );
    }
    if let Some(first) = changed.first() {
        let message = format!(
            "Column {}.{} is {} where Umami {} defines {}, although all {} migrations are recorded. The schema was changed by hand after migration. Change it back or restore a backup, and run the inspection again.",
            first["table"].as_str().unwrap_or_default(),
            first["column"].as_str().unwrap_or_default(),
            first["found"]["type"].as_str().unwrap_or_default(),
            layout.version(),
            first["expected"]["type"].as_str().unwrap_or_default(),
            expected.migrations.len()
        );
        return refuse(
            "column_changed",
            json!({ "migrationTableValid": true, "changed": changed }),
            message,
        );
    }
    Verdict::Accept(Accepted { layout, tolerated })
}

/// MySQL type families the importer's casts read alike (`source.rs`): `timestamp` and
/// `datetime` at any precision (read in the UTC session), an integer type of either sign, and
/// `char` or `varchar` of any length (cut to the snapshot's length, see [`width`]).
#[derive(Debug, PartialEq, Eq)]
enum Family<'a> {
    Time,
    Integer(&'a str),
    Text,
}

fn family(data_type: &str) -> Option<Family<'_>> {
    match data_type.split(['(', ' ']).next()? {
        "timestamp" | "datetime" => Some(Family::Time),
        base @ ("tinyint" | "smallint" | "mediumint" | "int" | "bigint") => {
            Some(Family::Integer(base))
        }
        "char" | "varchar" => Some(Family::Text),
        _ => None,
    }
}

/// The declared length of a text column of a layout: the longest value Umami writes there.
pub fn width(layout: Layout, table: &str, column: &str) -> Option<usize> {
    let (data_type, _) = layout.snapshot().tables.get(table)?.get(column)?;
    let length = ["varchar(", "char(", "character varying(", "character("]
        .iter()
        .find_map(|prefix| data_type.strip_prefix(prefix))?;
    length.strip_suffix(')')?.parse().ok()
}

fn refuse(code: &'static str, details: Value, message: String) -> Verdict {
    Verdict::Refuse(Refusal {
        code,
        details,
        message: format!("{message} {UNCHANGED}"),
    })
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// `14` of `14_add_link_and_pixel`.
fn prefix(name: Option<&&str>) -> String {
    name.and_then(|n| n.split('_').next())
        .unwrap_or_default()
        .to_owned()
}

/// The tables of a layout, by name.
pub fn tables(layout: Layout) -> Vec<&'static str> {
    layout
        .snapshot()
        .tables
        .keys()
        .map(String::as_str)
        .collect()
}

/// The number of migrations a layout records.
pub fn migration_count(layout: Layout) -> usize {
    layout.snapshot().migrations.len()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/import-schemas")
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// The fixture files as a source would report them.
    fn observed(dir: &Path, engine: Engine) -> Observed {
        let migrations = read(&dir.join("migrations.json"))
            .as_array()
            .unwrap()
            .iter()
            .map(|m| Migration {
                name: m["migration_name"].as_str().unwrap().into(),
                checksum: m["checksum"].as_str().unwrap().into(),
                finished: !m["finished_at"].is_null(),
                rolled_back: !m["rolled_back_at"].is_null(),
            })
            .collect();
        let columns = match dir.join("columns.json") {
            path if path.exists() => read(&path)["tables"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|t| {
                    t["columns"].as_array().unwrap().iter().map(|c| Column {
                        table: t["name"].as_str().unwrap().into(),
                        column: c["name"].as_str().unwrap().into(),
                        data_type: c["type"].as_str().unwrap().into(),
                        nullable: c["nullable"].as_bool().unwrap(),
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        Observed {
            engine,
            migrations,
            columns,
        }
    }

    fn engine(expected: &Value) -> Engine {
        match expected["engine"].as_str().unwrap() {
            "postgresql" => Engine::Postgresql,
            _ => Engine::Mysql,
        }
    }

    #[test]
    fn both_supported_snapshots_are_accepted() {
        for case in ["postgresql-3.4.0", "mysql-2.20.2"] {
            let dir = fixtures().join(case);
            let expected = read(&dir.join("expected.json"));
            let Verdict::Accept(accepted) = check(&observed(&dir, engine(&expected))) else {
                panic!("{case} refused");
            };
            assert_eq!(
                migration_count(accepted.layout),
                expected["migrationCount"].as_u64().unwrap() as usize
            );
            assert!(accepted.report().is_none(), "{case}");
        }
    }

    /// Sets the type of one snapshot column, the way a `prisma db push` or a hand edit would.
    fn retype(observed: &mut Observed, table: &str, column: &str, data_type: &str) {
        let c = observed
            .columns
            .iter_mut()
            .find(|c| c.table == table && c.column == column)
            .unwrap();
        c.data_type = data_type.into();
    }

    #[test]
    fn mysql_types_of_the_same_family_are_accepted_and_reported() {
        let mut source = observed(&fixtures().join("mysql-2.20.2"), Engine::Mysql);
        retype(&mut source, "website_event", "created_at", "datetime(3)");
        retype(&mut source, "website_event", "event_type", "int");
        retype(&mut source, "session", "region", "varchar(20)");
        retype(&mut source, "report", "type", "varchar(50)");
        let Verdict::Accept(accepted) = check(&source) else {
            panic!("refused");
        };
        let report = accepted.report().unwrap();
        assert_eq!(report["columns"].as_array().unwrap().len(), 4);
        assert!(report["message"].as_str().unwrap().contains("read as UTC"));
        assert_eq!(width(Layout::Umami2Mysql, "session", "region"), Some(20));
        assert_eq!(
            width(Layout::Umami3Postgresql, "session", "country"),
            Some(2)
        );
    }

    #[test]
    fn other_type_changes_still_refuse() {
        let mysql = fixtures().join("mysql-2.20.2");
        let postgres = fixtures().join("postgresql-3.4.0");
        let mut other_family = observed(&mysql, Engine::Mysql);
        retype(&mut other_family, "website_event", "url_path", "text");
        let mut wider_integer = observed(&mysql, Engine::Mysql);
        retype(&mut wider_integer, "website_event", "event_type", "bigint");
        let mut nullable = observed(&mysql, Engine::Mysql);
        nullable
            .columns
            .iter_mut()
            .find(|c| c.table == "website_event" && c.column == "url_path")
            .unwrap()
            .nullable = true;
        let mut postgres_length = observed(&postgres, Engine::Postgresql);
        retype(
            &mut postgres_length,
            "session",
            "region",
            "character varying(50)",
        );
        for source in [other_family, wider_integer, nullable, postgres_length] {
            let Verdict::Refuse(refusal) = check(&source) else {
                panic!("accepted");
            };
            assert_eq!(refusal.code, "column_changed");
        }
    }

    #[test]
    fn every_refusal_fixture_refuses_with_its_code_and_names() {
        let mut cases = 0;
        for entry in std::fs::read_dir(fixtures().join("refusal")).unwrap() {
            let dir = entry.unwrap().path();
            let expected = read(&dir.join("expected.json"));
            let Verdict::Refuse(refusal) = check(&observed(&dir, engine(&expected))) else {
                panic!("{} accepted", dir.display());
            };
            assert_eq!(refusal.code, expected["code"], "{}", dir.display());
            let mut details = expected["details"].clone();
            details.as_object_mut().unwrap().remove("note");
            assert_eq!(refusal.details, details, "{}", dir.display());
            for name in ["missing", "failed", "unknown"] {
                for item in details[name].as_array().into_iter().flatten() {
                    let named = item
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| item["column"].as_str().unwrap().to_owned());
                    if refusal.code != "unsupported_pairing" {
                        assert!(refusal.message.contains(&named), "{}", refusal.message);
                    }
                }
            }
            assert!(refusal.message.ends_with(UNCHANGED));
            cases += 1;
        }
        assert_eq!(cases, 10);
    }
}
