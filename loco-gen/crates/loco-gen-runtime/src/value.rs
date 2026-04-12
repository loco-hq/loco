#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(std::string::String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<Value>),
}

impl Value {
    pub fn as_string(&self) -> Option<String> {
        match self {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_value() {
        let v = Value::String("hello".to_string());
        assert_eq!(v.as_string(), Some("hello".to_string()));
        assert_eq!(v.as_integer(), None);
    }

    #[test]
    fn test_integer_value() {
        let v = Value::Integer(42);
        assert_eq!(v.as_integer(), Some(42));
        assert_eq!(v.as_string(), None);
    }

    #[test]
    fn test_float_value() {
        let v = Value::Float(3.14);
        assert_eq!(v.as_float(), Some(3.14));
    }

    #[test]
    fn test_boolean_value() {
        let v = Value::Boolean(true);
        assert_eq!(v.as_boolean(), Some(true));
    }
}
