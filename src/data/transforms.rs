//! Data transforms: filter, sort, and limit operations on JSON arrays.

use serde_json::Value;

/// Apply filter, sort, and limit transforms (in that order) to a JSON value.
///
/// If the value is not an array, it is returned as-is (transforms only apply
/// to arrays).
pub fn apply_transforms(
    value: Value,
    filter: &Option<std::collections::HashMap<String, String>>,
    sort: &Option<String>,
    limit: &Option<usize>,
) -> Value {
    let mut val = value;

    if let Some(filters) = filter {
        val = apply_filter(val, filters);
    }
    if let Some(sort_spec) = sort {
        val = apply_sort(val, sort_spec);
    }
    if let Some(max) = limit {
        val = apply_limit(val, *max);
    }

    val
}

/// Keep only items where `item[key] == value` for every (key, value) pair.
fn apply_filter(value: Value, filters: &std::collections::HashMap<String, String>) -> Value {
    match value {
        Value::Array(arr) => {
            let filtered: Vec<Value> = arr
                .into_iter()
                .filter(|item| {
                    filters.iter().all(|(key, expected)| {
                        match item.get(key) {
                            Some(val) => value_matches_string(val, expected),
                            None => false,
                        }
                    })
                })
                .collect();
            Value::Array(filtered)
        }
        other => other,
    }
}

/// Check whether a JSON value matches a string representation.
///
/// - String values are compared directly.
/// - Numbers are compared by their string representation.
/// - Booleans are compared as "true"/"false".
/// - Other types never match.
fn value_matches_string(val: &Value, expected: &str) -> bool {
    match val {
        Value::String(s) => s == expected,
        Value::Number(n) => n.to_string() == expected,
        Value::Bool(b) => b.to_string() == expected,
        _ => false,
    }
}

/// Sort an array of objects by a field.
///
/// Sort specification:
/// - `"field"` → ascending
/// - `"-field"` → descending
fn apply_sort(value: Value, sort_spec: &str) -> Value {
    match value {
        Value::Array(mut arr) => {
            let (field, descending) = if let Some(stripped) = sort_spec.strip_prefix('-') {
                (stripped, true)
            } else {
                (sort_spec, false)
            };

            arr.sort_by(|a, b| {
                let va = a.get(field);
                let vb = b.get(field);
                let cmp = compare_values(va, vb);
                if descending { cmp.reverse() } else { cmp }
            });

            Value::Array(arr)
        }
        other => other,
    }
}

/// Compare two optional JSON values for sorting.
///
/// Ordering: `None` < `Null` < `Bool` < `Number` < `String` < other.
/// Within the same type, standard comparisons apply.
fn compare_values(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(va), Some(vb)) => {
            match (va, vb) {
                (Value::Number(na), Value::Number(nb)) => {
                    let fa = na.as_f64().unwrap_or(0.0);
                    let fb = nb.as_f64().unwrap_or(0.0);
                    fa.partial_cmp(&fb).unwrap_or(Ordering::Equal)
                }
                (Value::String(sa), Value::String(sb)) => sa.cmp(sb),
                (Value::Bool(ba), Value::Bool(bb)) => ba.cmp(bb),
                _ => {
                    // Fall back to string representation comparison.
                    let sa = va.to_string();
                    let sb = vb.to_string();
                    sa.cmp(&sb)
                }
            }
        }
    }
}

/// Truncate an array to at most `max` items.
fn apply_limit(value: Value, max: usize) -> Value {
    match value {
        Value::Array(mut arr) => {
            arr.truncate(max);
            Value::Array(arr)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_filter_by_single_key() {
        let data = json!([
            {"name": "Alice", "role": "admin"},
            {"name": "Bob", "role": "user"},
            {"name": "Carol", "role": "admin"},
        ]);
        let mut filters = HashMap::new();
        filters.insert("role".into(), "admin".into());

        let result = apply_filter(data, &filters);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "Alice");
        assert_eq!(arr[1]["name"], "Carol");
    }

    #[test]
    fn test_filter_by_multiple_keys() {
        let data = json!([
            {"name": "Alice", "role": "admin", "active": "true"},
            {"name": "Bob", "role": "admin", "active": "false"},
            {"name": "Carol", "role": "user", "active": "true"},
        ]);
        let mut filters = HashMap::new();
        filters.insert("role".into(), "admin".into());
        filters.insert("active".into(), "true".into());

        let result = apply_filter(data, &filters);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "Alice");
    }

    #[test]
    fn test_filter_numeric_value() {
        let data = json!([
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"},
            {"id": 3, "name": "Carol"},
        ]);
        let mut filters = HashMap::new();
        filters.insert("id".into(), "2".into());

        let result = apply_filter(data, &filters);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "Bob");
    }

    #[test]
    fn test_filter_no_match() {
        let data = json!([
            {"name": "Alice", "role": "admin"},
        ]);
        let mut filters = HashMap::new();
        filters.insert("role".into(), "superadmin".into());

        let result = apply_filter(data, &filters);
        let arr = result.as_array().unwrap();
        assert!(arr.is_empty());
    }

    #[test]
    fn test_filter_non_array_passthrough() {
        let data = json!({"key": "value"});
        let filters = HashMap::new();
        let result = apply_filter(data.clone(), &filters);
        assert_eq!(result, data);
    }

    #[test]
    fn test_sort_ascending_string() {
        let data = json!([
            {"name": "Carol"},
            {"name": "Alice"},
            {"name": "Bob"},
        ]);
        let result = apply_sort(data, "name");
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["name"], "Alice");
        assert_eq!(arr[1]["name"], "Bob");
        assert_eq!(arr[2]["name"], "Carol");
    }

    #[test]
    fn test_sort_descending_string() {
        let data = json!([
            {"name": "Alice"},
            {"name": "Carol"},
            {"name": "Bob"},
        ]);
        let result = apply_sort(data, "-name");
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["name"], "Carol");
        assert_eq!(arr[1]["name"], "Bob");
        assert_eq!(arr[2]["name"], "Alice");
    }

    #[test]
    fn test_sort_ascending_numeric() {
        let data = json!([
            {"id": 3, "name": "Carol"},
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"},
        ]);
        let result = apply_sort(data, "id");
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["name"], "Alice");
        assert_eq!(arr[1]["name"], "Bob");
        assert_eq!(arr[2]["name"], "Carol");
    }

    #[test]
    fn test_sort_descending_numeric() {
        let data = json!([
            {"id": 1, "name": "Alice"},
            {"id": 3, "name": "Carol"},
            {"id": 2, "name": "Bob"},
        ]);
        let result = apply_sort(data, "-id");
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["name"], "Carol");
        assert_eq!(arr[1]["name"], "Bob");
        assert_eq!(arr[2]["name"], "Alice");
    }

    #[test]
    fn test_sort_non_array_passthrough() {
        let data = json!({"key": "value"});
        let result = apply_sort(data.clone(), "key");
        assert_eq!(result, data);
    }

    #[test]
    fn test_sort_missing_field() {
        let data = json!([
            {"name": "Alice"},
            {"name": "Bob", "age": 30},
            {"age": 25},
        ]);
        // Items without the sort field come first (None < Some)
        let result = apply_sort(data, "age");
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["name"], "Alice"); // no age field
        assert_eq!(arr[1]["age"], 25);
        assert_eq!(arr[2]["age"], 30);
    }

    #[test]
    fn test_limit() {
        let data = json!([1, 2, 3, 4, 5]);
        let result = apply_limit(data, 3);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr, &[json!(1), json!(2), json!(3)]);
    }

    #[test]
    fn test_limit_larger_than_array() {
        let data = json!([1, 2]);
        let result = apply_limit(data, 10);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn test_limit_zero() {
        let data = json!([1, 2, 3]);
        let result = apply_limit(data, 0);
        let arr = result.as_array().unwrap();
        assert!(arr.is_empty());
    }

    #[test]
    fn test_limit_non_array_passthrough() {
        let data = json!("hello");
        let result = apply_limit(data.clone(), 5);
        assert_eq!(result, data);
    }

    #[test]
    fn test_apply_transforms_all() {
        let data = json!([
            {"id": 5, "status": "active", "name": "Eve"},
            {"id": 3, "status": "active", "name": "Carol"},
            {"id": 1, "status": "inactive", "name": "Alice"},
            {"id": 4, "status": "active", "name": "Dave"},
            {"id": 2, "status": "active", "name": "Bob"},
        ]);
        let mut filter = HashMap::new();
        filter.insert("status".into(), "active".into());
        let sort = Some("id".into());
        let limit = Some(2usize);

        let result = apply_transforms(data, &Some(filter), &sort, &limit);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "Bob");   // id=2
        assert_eq!(arr[1]["name"], "Carol"); // id=3
    }

    #[test]
    fn test_apply_transforms_none() {
        let data = json!([1, 2, 3]);
        let result = apply_transforms(data.clone(), &None, &None, &None);
        assert_eq!(result, data);
    }
}
