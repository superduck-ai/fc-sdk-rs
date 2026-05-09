pub fn bool_value(value: Option<bool>) -> bool {
    value.unwrap_or(false)
}

pub fn bool_ptr(value: bool) -> Option<bool> {
    Some(value)
}

pub fn string_value(value: Option<&str>) -> String {
    value.unwrap_or_default().to_string()
}

pub fn string_ptr(value: impl Into<String>) -> Option<String> {
    Some(value.into())
}

pub fn int64_ptr(value: i64) -> Option<i64> {
    Some(value)
}

pub fn int64_value(value: Option<i64>) -> i64 {
    value.unwrap_or_default()
}

pub fn int_ptr(value: i32) -> Option<i32> {
    Some(value)
}

pub fn int_value(value: Option<i32>) -> i32 {
    value.unwrap_or_default()
}
