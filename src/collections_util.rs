/// Creates a HashSet with String::from(...)
///
/// ```
/// let s = string_set!["a", "b", "c"];
/// let mut expected = std::collections::HashSet::new();
/// expected.insert(String::from("a"));
/// expected.insert(String::from("b"));
/// expected.insert(String::from("c"));
/// assert_eq!(s, expected);
/// ``````
#[macro_export]
macro_rules! string_set {
    ($($elm:literal),*) => {{
        use std::collections::HashSet;

        #[allow(unused_mut)]
        let mut s: HashSet<String> = std::collections::HashSet::new();
        $(
            s.insert(String::from($elm));
        )*
        s
    }};
}

#[cfg(test)]
mod test {
    #[test]
    pub fn string_set_doctest() {
        let s = string_set!["a", "b", "c"];
        let mut expected = std::collections::HashSet::new();
        expected.insert(String::from("a"));
        expected.insert(String::from("b"));
        expected.insert(String::from("c"));
        assert_eq!(s, expected);
    }

    #[test]
    pub fn string_set_empty() {
        let s = string_set![];
        let expected = std::collections::HashSet::new();
        assert_eq!(s, expected);
    }
}
