//! Query sources, in Helix's layout (`queries/<lang>/<kind>.scm`), with `; inherits: a,b`
//! lines replaced in place by the named directories' queries of the same kind.

macro_rules! table {
    ($($dir:literal / $kind:literal),* $(,)?) => {
        const TABLE: &[(&str, &str, &str)] = &[
            $(($dir, $kind, include_str!(concat!("../queries/", $dir, "/", $kind, ".scm")))),*
        ];
    };
}

table! {
    "_javascript"/"highlights", "_javascript"/"injections", "_javascript"/"folds",
    "_javascript"/"indents", "_javascript"/"brackets",
    "_jsx"/"highlights", "_jsx"/"folds", "_jsx"/"indents",
    "javascript"/"highlights", "javascript"/"injections", "javascript"/"folds",
    "javascript"/"indents", "javascript"/"brackets",
    "typescript"/"highlights", "typescript"/"injections", "typescript"/"folds",
    "typescript"/"indents", "typescript"/"brackets",
    "tsx"/"highlights", "tsx"/"injections", "tsx"/"folds", "tsx"/"indents", "tsx"/"brackets",
    "css"/"highlights", "css"/"folds", "css"/"indents", "css"/"brackets",
    "svelte"/"highlights", "svelte"/"injections", "svelte"/"folds", "svelte"/"indents",
    "svelte"/"brackets",
    "toml"/"highlights", "toml"/"folds", "toml"/"indents", "toml"/"brackets",
}

/// The raw text of one query file, if it exists.
pub(crate) fn raw(dir: &str, kind: &str) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(d, k, _)| *d == dir && *k == kind)
        .map(|(_, _, s)| *s)
}

/// The query of `kind` for directory `dir` with inherits resolved; `None` if neither the file
/// nor anything it inherits exists.
pub(crate) fn resolve(dir: &str, kind: &str) -> Option<String> {
    let src = raw(dir, kind)?;
    let mut out = String::with_capacity(src.len());
    for line in src.split_inclusive('\n') {
        match inherits(line) {
            Some(list) => {
                for parent in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    if let Some(q) = resolve(parent, kind) {
                        out.push_str(&q);
                        out.push('\n');
                    }
                }
            }
            None => out.push_str(line),
        }
    }
    Some(out)
}

fn inherits(line: &str) -> Option<&str> {
    let rest = line
        .trim_start()
        .strip_prefix(';')?
        .trim_start_matches(';')
        .trim_start();
    let rest = rest.strip_prefix("inherits")?.trim_start();
    Some(rest.strip_prefix(':').unwrap_or(rest).trim())
}
