//! Small helpers for walking InnerTube's deeply nested JSON without panicking.

use serde_json::Value;

/// Follows `path` through objects (by key) and arrays (by numeric segment).
pub fn path<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path {
        cur = match seg.parse::<usize>() {
            Ok(i) if cur.is_array() => cur.get(i)?,
            _ => cur.get(*seg)?,
        };
    }
    Some(cur)
}

pub fn str_at<'a>(v: &'a Value, p: &[&str]) -> Option<&'a str> {
    path(v, p)?.as_str()
}

/// Collects every value stored under `key` anywhere in the tree (depth-first).
pub fn find_all<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == key {
                    out.push(child);
                }
                find_all(child, key, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|c| find_all(c, key, out)),
        _ => {}
    }
}

pub fn find_first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|c| find_first(c, key))
        }
        Value::Array(items) => items.iter().find_map(|c| find_first(c, key)),
        _ => None,
    }
}

/// Concatenated text of a `{"runs":[{"text":..}]}` (or `{"simpleText":..}`) node.
pub fn runs_text(v: &Value) -> String {
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return s.to_owned();
    }
    v.get("runs")
        .and_then(Value::as_array)
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

/// "3:41" → 221, "1:02:03" → 3723.
pub fn parse_duration(s: &str) -> Option<u32> {
    let mut total = 0u32;
    let mut parts = 0;
    for p in s.trim().split(':') {
        total = total.checked_mul(60)?.checked_add(p.parse::<u32>().ok()?)?;
        parts += 1;
    }
    (2..=3).contains(&parts).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn path_walks_objects_and_arrays() {
        let v = json!({"a": [{"b": "x"}, {"b": "y"}]});
        assert_eq!(str_at(&v, &["a", "1", "b"]), Some("y"));
        assert!(path(&v, &["a", "5"]).is_none());
        assert!(path(&v, &["z"]).is_none());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("3:41"), Some(221));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("1.2B plays"), None);
        assert_eq!(parse_duration("12"), None);
    }

    #[test]
    fn find_helpers() {
        let v = json!({"x": {"k": 1}, "y": [{"k": 2}]});
        let mut out = vec![];
        find_all(&v, "k", &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(find_first(&v, "k"), Some(&json!(1)));
    }

    #[test]
    fn runs() {
        assert_eq!(
            runs_text(&json!({"runs":[{"text":"a"},{"text":"b"}]})),
            "ab"
        );
        assert_eq!(runs_text(&json!({"simpleText":"z"})), "z");
        assert_eq!(runs_text(&json!(null)), "");
    }
}
