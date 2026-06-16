use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum FilterCondition {
    Eq(Value),
    Ne(Value),
    Lt(Value),
    Lte(Value),
    Gt(Value),
    Gte(Value),
    Like(String),
    NotLike(String),
    In(Vec<Value>),
    NotIn(Vec<Value>),
    IsSet,
    IsNotSet,
}

impl FilterCondition {
    /// Returns (sql_fragment, bound_params).
    /// `next_ph` is called once per bound parameter to get the placeholder token ("?" or "$N").
    pub fn to_sql(&self, col: &str, mut next_ph: impl FnMut() -> String) -> (String, Vec<Value>) {
        match self {
            FilterCondition::Eq(v) => (format!("\"{}\" = {}", col, next_ph()), vec![v.clone()]),
            FilterCondition::Ne(v) => (format!("\"{}\" != {}", col, next_ph()), vec![v.clone()]),
            FilterCondition::Lt(v) => (format!("\"{}\" < {}", col, next_ph()), vec![v.clone()]),
            FilterCondition::Lte(v) => (format!("\"{}\" <= {}", col, next_ph()), vec![v.clone()]),
            FilterCondition::Gt(v) => (format!("\"{}\" > {}", col, next_ph()), vec![v.clone()]),
            FilterCondition::Gte(v) => (format!("\"{}\" >= {}", col, next_ph()), vec![v.clone()]),
            FilterCondition::Like(s) => (
                format!("\"{}\" LIKE {}", col, next_ph()),
                vec![Value::String(s.clone())],
            ),
            FilterCondition::NotLike(s) => (
                format!("\"{}\" NOT LIKE {}", col, next_ph()),
                vec![Value::String(s.clone())],
            ),
            FilterCondition::IsSet => (format!("\"{}\" IS NOT NULL", col), vec![]),
            FilterCondition::IsNotSet => (format!("\"{}\" IS NULL", col), vec![]),

            FilterCondition::In(vals) => {
                // NULL values in an IN list must use IS NULL because NULL IN (...) never matches.
                let (nulls, scalars): (Vec<_>, Vec<_>) = vals.iter().partition(|v| v.is_null());
                let mut scalar_phs = Vec::new();
                let mut scalar_params = Vec::new();
                for v in &scalars {
                    scalar_phs.push(next_ph());
                    scalar_params.push((*v).clone());
                }

                let mut parts = Vec::new();
                if !scalar_phs.is_empty() {
                    parts.push(format!("\"{}\" IN ({})", col, scalar_phs.join(", ")));
                }
                if !nulls.is_empty() {
                    parts.push(format!("\"{}\" IS NULL", col));
                }
                let frag = if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    format!("({})", parts.join(" OR "))
                };
                (frag, scalar_params)
            }

            FilterCondition::NotIn(vals) => {
                let (nulls, scalars): (Vec<_>, Vec<_>) = vals.iter().partition(|v| v.is_null());
                let mut scalar_phs = Vec::new();
                let mut scalar_params = Vec::new();
                for v in &scalars {
                    scalar_phs.push(next_ph());
                    scalar_params.push((*v).clone());
                }

                let mut parts = Vec::new();
                if !scalar_phs.is_empty() {
                    parts.push(format!("\"{}\" NOT IN ({})", col, scalar_phs.join(", ")));
                }
                if !nulls.is_empty() {
                    parts.push(format!("\"{}\" IS NOT NULL", col));
                }
                let frag = if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    format!("({})", parts.join(" AND "))
                };
                (frag, scalar_params)
            }
        }
    }
}

impl<'de> Deserialize<'de> for FilterCondition {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(parse_filter_condition(value))
    }
}

fn parse_filter_condition(val: Value) -> FilterCondition {
    if let Value::Array(ref arr) = val {
        if arr.len() == 2 {
            let op = arr[0].as_str().unwrap_or("").to_lowercase();
            let operand = arr[1].clone();
            return match op.as_str() {
                "=" => FilterCondition::Eq(operand),
                "!=" => FilterCondition::Ne(operand),
                ">" => FilterCondition::Gt(operand),
                ">=" => FilterCondition::Gte(operand),
                "<" => FilterCondition::Lt(operand),
                "<=" => FilterCondition::Lte(operand),
                "like" => FilterCondition::Like(operand.as_str().unwrap_or("").to_string()),
                "not like" => FilterCondition::NotLike(operand.as_str().unwrap_or("").to_string()),
                "in" => {
                    let items = operand.as_array().cloned().unwrap_or_default();
                    FilterCondition::In(items)
                }
                "not in" => {
                    let items = operand.as_array().cloned().unwrap_or_default();
                    FilterCondition::NotIn(items)
                }
                "is" => match operand.as_str().unwrap_or("").to_lowercase().as_str() {
                    "set" => FilterCondition::IsSet,
                    "not set" => FilterCondition::IsNotSet,
                    _ => FilterCondition::Eq(operand),
                },
                _ => FilterCondition::Eq(val),
            };
        }
    }
    FilterCondition::Eq(val)
}
