use std::{
    collections::{HashMap, HashSet},
    fs,
};

use crate::util::simplify_result;

pub struct Config {
    pub multivalue_keys: HashSet<String>,
}

#[derive(PartialEq, Debug)]
pub struct Contents {
    pub single_value: HashMap<String, String>,
    pub multi_value: HashMap<String, Vec<String>>,
}

impl Config {
    pub fn single_value_only() -> Config {
        Config {
            multivalue_keys: HashSet::new(),
        }
    }

    /// Reads a simple tab separated file and inserts the key/value pairs in a
    /// HashMap.
    pub fn read_file(&self, path: &str) -> Result<Contents, String> {
        let data = simplify_result(String::from_utf8(simplify_result(fs::read(path))?))?;
        match self.read_string(&data) {
            Err(e) => Err(format!(
                "Failed to parse contents of file '{}': {}",
                path, e
            )),
            Ok(x) => Ok(x),
        }
    }

    pub fn read_string(&self, data: &str) -> Result<Contents, String> {
        let mut single_value: HashMap<String, String> = HashMap::new();
        let mut multi_value: HashMap<String, Vec<String>> = HashMap::new();

        for line in data.split('\n') {
            if line.is_empty() {
                continue;
            }

            match line.find('\t') {
                None => return Err(String::from("Corrupted")),
                Some(i) => {
                    let key = unescape_string(&line[..i])?;
                    let val = unescape_string(&line[i + 1..])?;
                    if self.multivalue_keys.contains(&key) {
                        let list = multi_value.entry(key).or_insert(Vec::new());
                        list.push(String::from(val));
                    } else {
                        if single_value.contains_key(&key) {
                            return Err(format!(
                                "Multiple values found for key '{}', however, the key is not defined as multivalued.",
                                key
                            ));
                        } else {
                            single_value.insert(key, val);
                        }
                    }
                }
            }
        }

        Ok(Contents {
            single_value,
            multi_value,
        })
    }
}

impl Contents {
    pub fn write_file(&self, path: &str) -> Result<(), String> {
        simplify_result(fs::write(path, self.write_string()?))
    }

    pub fn write_string(&self) -> Result<String, String> {
        let mut sorted_singles = self.single_value.iter().collect::<Vec<_>>();
        sorted_singles.sort();

        let mut result = String::new();

        for item in sorted_singles {
            result.push_str(&escape_string(item.0));
            result.push('\t');
            result.push_str(&escape_string(item.1));
            result.push('\n');
        }

        let mut sorted_multis = self.multi_value.iter().collect::<Vec<_>>();
        sorted_multis.sort();

        for item in sorted_multis {
            if self.single_value.contains_key(item.0) {
                return Err(format!(
                    "Serialization failed: Key {} is specified as both multi-value and single-value",
                    item.0
                ));
            }

            let key_escaped = escape_string(item.0);
            for val in item.1 {
                result.push_str(&key_escaped);
                result.push('\t');
                result.push_str(&escape_string(val));
                result.push('\n');
            }
        }

        Ok(if result.is_empty() {
            String::from("\n")
        } else {
            result
        })
    }
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape_string(s: &str) -> Result<String, String> {
    let mut final_str = String::with_capacity(s.len());
    let mut is_escaped = false;

    for ch in s.chars() {
        if is_escaped {
            is_escaped = false;

            match ch {
                '\\' => {
                    final_str.push(ch);
                }
                'n' => {
                    final_str.push('\n');
                }
                _ => {
                    return Err(format!(
                        "Failed to unescape string, '\\{}' is not a valid escape sequence.",
                        ch
                    ));
                }
            }
        } else if ch == '\\' {
            is_escaped = true;
        } else {
            final_str.push(ch);
        }
    }

    if is_escaped {
        Err(String::from("Failed to unescape string: trailing '\\'"))
    } else {
        Ok(final_str)
    }
}

#[cfg(test)]
mod tests {
    use core::panic;
    use std::collections::{HashMap, HashSet};

    use crate::tab_separated_key_value::unescape_string;

    use super::{Config, Contents, escape_string};

    #[test]
    fn read_tskv() {
        let lit = "a\tb
b\tc
d\te
a\tf
a\tasdfsafd\tasdfAF!!\\nasdf
g\t\tasdf\t\\\\\\nfdsa

aa\t1
aa\t2";
        let res = Config {
            multivalue_keys: {
                let mut s = HashSet::new();
                s.insert(String::from("a"));
                s.insert(String::from("aa"));
                s
            },
        }
        .read_string(lit);

        match res {
            Err(e) => panic!("{}", e),
            Ok(data) => {
                assert_eq!(data.single_value.get("a"), None);
                assert_eq!(data.single_value.get("aaa"), None);
                assert_eq!(data.single_value.get("b"), Some(&String::from("c")));
                assert_eq!(data.single_value.get("d"), Some(&String::from("e")));
                assert_eq!(
                    data.single_value.get("g"),
                    Some(&String::from("\tasdf\t\\\nfdsa"))
                );
                assert_eq!(
                    data.multi_value.get("a"),
                    Some(&vec![
                        String::from("b"),
                        String::from("f"),
                        String::from("asdfsafd\tasdfAF!!\nasdf")
                    ])
                );
                assert_eq!(
                    data.multi_value.get("aa"),
                    Some(&vec![String::from("1"), String::from("2")])
                );
            }
        }
    }

    #[test]
    fn read_written_tskv() {
        let initial_contents = Contents {
            single_value: {
                let mut s = HashMap::new();
                s.insert(String::from("a"), String::from("b"));
                s.insert(String::from("b"), String::from("asdf\tasdf"));
                s.insert(
                    String::from("c"),
                    String::from("asdf\nasdf\tasdfjlk\\\\njsfkd"),
                );
                s.insert(String::from("a\\n"), String::from("weird key"));
                s.insert(String::from("a\nb"), String::from("weird key"));
                s
            },
            multi_value: {
                let mut s = HashMap::new();
                s.insert(
                    String::from("d"),
                    vec![
                        String::from("data data"),
                        String::from("data data\nasdfasdf"),
                        String::from("asdfasdf"),
                        String::from("asdfasdf"),
                    ],
                );
                s.insert(String::from("e"), vec![String::from("asdf\tasdf")]);
                s.insert(
                    String::from("f"),
                    vec![String::from("a"), String::from("a")],
                );
                s.insert(
                    String::from("g\\n"),
                    vec![String::from("weird key"), String::from("very weird")],
                );
                s.insert(
                    String::from("g\n"),
                    vec![String::from("wow weird"), String::from("such weird")],
                );
                s
            },
        };

        let written_string = initial_contents.write_string().unwrap();

        let read_result = Config {
            multivalue_keys: {
                let mut s = HashSet::new();
                s.insert(String::from("d"));
                s.insert(String::from("e"));
                s.insert(String::from("f"));
                s.insert(String::from("ff"));
                s.insert(String::from("g\n"));
                s.insert(String::from("g\\n"));
                s
            },
        }
        .read_string(&written_string);

        match read_result {
            Err(e) => panic!("{}", e),
            Ok(data) => {
                assert_eq!(data, initial_contents);
            }
        }
    }

    #[test]
    fn read_invalid_tskv_no_multivalue() {
        let config = Config {
            multivalue_keys: HashSet::new(),
        };

        let to_test = vec![
            // fails since a is specified multiple times
            "a\tb\na\tc",
            // fails since b is specified multiple times
            "a\tb\nb\tc\nc\tc\nb\td",
            // fails since escape sequence in key is invalid
            "a\\bn\tasdf",
            // fails since escape sequence in value is invalid
            "a\\\\bn\ta\\sdf",
        ];

        for s in to_test {
            match config.read_string(s) {
                Err(_) => {}
                Ok(_) => panic!("Expected failure but successfully read:\n{}", s),
            }
        }
    }

    #[test]
    fn read_invalid_tskv_with_multivalue() {
        let config = Config {
            multivalue_keys: {
                let mut s = HashSet::new();
                s.insert(String::from("c\\"));
                s
            },
        };

        let to_test = vec![
            // fails since a is specified multiple times
            "a\tb\na\tc",
            // fails since b is specified multiple times
            "a\tb\nb\tc\nc\tc\nb\td",
            // fails since escape sequence in key and value are invalid
            "a\\bn\tas\\df",
            // fails since escape sequence in key is invalid
            "c\\\td",
            // fails since escape sequence in value is invalid
            "c\\\\\td\\",
            // fails since escape sequence in second value is invalid
            "c\\\\\td\\nc\\\\\td\\c\\\\\td\\n",
        ];

        for s in to_test {
            match config.read_string(s) {
                Err(_) => {}
                Ok(_) => panic!("Expected failure but successfully read:\n{}", s),
            }
        }
    }

    #[test]
    fn write_invalid_tskv_overlap_single_multi() {
        // fails since the same name is used for single and multivalues keys
        let contents = Contents {
            single_value: {
                let mut m = HashMap::new();
                m.insert(String::from("a"), String::from("b"));
                m
            },
            multi_value: {
                let mut m = HashMap::new();
                m.insert(String::from("a"), vec![String::from("b")]);
                m
            },
        };

        match contents.write_string() {
            Err(_) => {}
            Ok(res) => panic!("Expected failure but successfully serialized:\n{}", res),
        }
    }

    #[test]
    fn escape_test() {
        assert_eq!(escape_string(""), "");
        assert_eq!(escape_string("\t"), "\t");
        assert_eq!(escape_string("\n"), "\\n");
        assert_eq!(escape_string("\\"), "\\\\");
        assert_eq!(escape_string("\\n"), "\\\\n");
        assert_eq!(
            escape_string("Tabs (\t) are not escaped"),
            "Tabs (\t) are not escaped"
        );
        assert_eq!(
            escape_string("This is a message\nthat a user might type"),
            "This is a message\\nthat a user might type"
        );
        assert_eq!(
            escape_string("A backslash (\\) is a character."),
            "A backslash (\\\\) is a character."
        );
        assert_eq!(
            escape_string("Backslash (\\) and n indicate a newline (\\n)\nThat was a newline."),
            "Backslash (\\\\) and n indicate a newline (\\\\n)\\nThat was a newline."
        );
        assert_eq!(escape_string("\\n\n\\n"), "\\\\n\\n\\\\n");
    }

    #[test]
    fn unescape_test() {
        let to_test = vec![
            "",
            "\t",
            "\n",
            "\\",
            "\\n",
            "Tabs (\t) are not escaped",
            "This is a message\nthat a user might type",
            "A backslash (\\) is a character.",
            "Backslash (\\) and n indicate a newline (\\n)\nThat was a newline.",
            "\\n\n\\n",
            "\\\\\\\\nn\\\\\\\\\\afs\\\\\n\\n\n\n\n\\\nnn\n\\n\\nffsdf\n\n\n\n\\n\n\n\\nnn\\\\",
            "a\n",
            "a\\",
            "\\a",
            "\na",
            "\\a\n",
            "\na\\",
            "\tNothing needs to be escaped here\t",
        ];

        for s in to_test {
            match unescape_string(&escape_string(s)) {
                Err(error) => panic!(
                    "Error when unescaping string generated by escape_string: {}",
                    error
                ),
                Ok(escaped) => assert_eq!(escaped, s),
            }
        }
    }

    #[test]
    fn unescape_fail_cases() {
        // every string contains an invalid escape sequence (a backslash followed by a character that is not '\' nor 'n')
        let to_test = vec![
            "\\a",
            "\\b",
            "asdfasdf\\asdfasdf",
            "asdfasdf\\",
            "\\n\\\\\\a\\n",
        ];

        for s in to_test {
            match unescape_string(s) {
                Err(_) => {}
                Ok(result) => panic!("Successfully unescaped bad string '{}' to '{}'", s, result),
            }
        }
    }
}
