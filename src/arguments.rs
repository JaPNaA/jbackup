use std::collections::{HashMap, HashSet, VecDeque};

pub struct Parser {
    flags: HashSet<String>,
    options: HashSet<String>,
}

impl Parser {
    pub fn new() -> Parser {
        Parser {
            flags: HashSet::new(),
            options: HashSet::new(),
        }
    }

    pub fn flag(&mut self, name: &str) -> &mut Parser {
        self.flags.insert(String::from(name));
        self
    }

    pub fn option(&mut self, name: &str) -> &mut Parser {
        self.options.insert(String::from(name));
        self
    }

    pub fn parse(&self, args_iter: impl Iterator<Item = String>) -> Arguments {
        let mut args = Arguments {
            flags: HashSet::new(),
            options: HashMap::new(),
            normal: VecDeque::new(),
        };

        let mut option_name = None;

        for s in args_iter {
            match option_name.take() {
                Some(k) => {
                    args.options.insert(k, s);
                }
                None => {
                    if self.flags.contains(&s) {
                        args.flags.insert(s);
                    } else if self.options.contains(&s) {
                        option_name.replace(s);
                    } else {
                        args.normal.push_back(s);
                    }
                }
            }
        }

        args
    }
}

pub struct Arguments {
    pub flags: HashSet<String>,
    pub options: HashMap<String, String>,
    pub normal: VecDeque<String>,
}

#[cfg(test)]
mod test {
    use crate::arguments::Parser;

    #[test]
    pub fn parses_options() {
        assert_eq!(
            Parser::new()
                .option("a")
                .parse(vec![String::from("a"), String::from("b")].into_iter())
                .options
                .get("a"),
            Some(&String::from("b"))
        );
    }
}
