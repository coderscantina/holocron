//! `matches` globs (0022 Funnel v2 and glob matching): `*` matches any run of characters
//! except `/`, `**` any run including `/`, `\` escapes the next character. A glob matches the
//! whole value, case-sensitively. It compiles to an RE2 pattern for `regexp_full_match`, bound
//! as a parameter, with its literal prefix for a cheap `starts_with` test first.

use holocron_protocol::limits::{GLOB_MAX_BYTES, GLOB_WILDCARDS_MAX};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    /// The literal text before the first wildcard; empty when the glob starts with one.
    pub prefix: String,
    /// The RE2 pattern matching exactly the values the glob matches.
    pub regex: String,
}

/// Compiles a glob, or names why it is refused.
pub fn parse(glob: &str) -> Result<Glob, &'static str> {
    if glob.is_empty() || glob.len() > GLOB_MAX_BYTES {
        return Err("a glob is 1 to 500 bytes");
    }
    // `.` must cross newlines too, since `**` matches any run.
    let mut regex = String::from("(?s)");
    let mut prefix = String::new();
    let mut literal = true;
    let mut wildcards = 0;
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        let text = match c {
            '*' => {
                wildcards += 1;
                literal = false;
                if chars.next_if_eq(&'*').is_some() {
                    regex.push_str(".*");
                } else {
                    regex.push_str("[^/]*");
                }
                continue;
            }
            '\\' => chars.next().ok_or("a trailing backslash escapes nothing")?,
            c => c,
        };
        if literal {
            prefix.push(text);
        }
        if "\\.+*?()|[]{}^$".contains(text) {
            regex.push('\\');
        }
        regex.push(text);
    }
    if wildcards > GLOB_WILDCARDS_MAX {
        return Err("a glob has at most 8 wildcards");
    }
    Ok(Glob { prefix, regex })
}

#[cfg(test)]
mod tests {
    use holocron_storage::duckdb::Connection;

    use super::parse;

    /// Which of `values` the glob matches in DuckDB, through the same bound pattern.
    fn matched<'a>(conn: &Connection, glob: &str, values: &[&'a str]) -> Vec<&'a str> {
        let glob = parse(glob).unwrap();
        values
            .iter()
            .copied()
            .filter(|value| {
                conn.query_row(
                    "SELECT starts_with(?, ?) AND regexp_full_match(?, ?)",
                    [*value, glob.prefix.as_str(), *value, glob.regex.as_str()],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn globs_match_whole_values_with_one_and_two_stars_and_escapes() {
        let conn = Connection::open_in_memory().unwrap();
        let paths = [
            "/blog",
            "/blog/",
            "/blog/a",
            "/blog/a/b",
            "/Blog/a",
            "/blog/a.html",
            "/blogxa",
            "/x/blog/a",
            "/docs/*",
            "/docs/a",
            "/a+b(c)",
            "/a\nb",
        ];
        assert_eq!(
            matched(&conn, "/blog/*", &paths),
            ["/blog/", "/blog/a", "/blog/a.html"]
        );
        assert_eq!(
            matched(&conn, "/blog/**", &paths),
            ["/blog/", "/blog/a", "/blog/a/b", "/blog/a.html"]
        );
        assert_eq!(
            matched(&conn, "**/a", &paths),
            ["/blog/a", "/Blog/a", "/x/blog/a", "/docs/a"]
        );
        assert_eq!(matched(&conn, "/blog", &paths), ["/blog"]);
        assert_eq!(matched(&conn, "/blog/*.html", &paths), ["/blog/a.html"]);
        assert_eq!(matched(&conn, r"/docs/\*", &paths), ["/docs/*"]);
        assert_eq!(matched(&conn, "/a+b(c)", &paths), ["/a+b(c)"]);
        assert_eq!(matched(&conn, "/a**", &paths), ["/a+b(c)", "/a\nb"]);
    }

    #[test]
    fn globs_are_bounded() {
        assert_eq!(parse("").unwrap_err(), "a glob is 1 to 500 bytes");
        assert!(parse(&"a".repeat(500)).is_ok());
        assert!(parse(&"a".repeat(501)).is_err());
        assert!(parse(&"*a".repeat(8)).is_ok());
        assert_eq!(
            parse(&"**a".repeat(9)).unwrap_err(),
            "a glob has at most 8 wildcards"
        );
        assert!(parse(r"/a\").is_err());
        assert_eq!(parse(r"/a\*b*").unwrap().prefix, "/a*b");
    }
}
