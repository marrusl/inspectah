//! Dependency expressions as rpm records them, plain and rich (boolean).
//!
//! A plain dependency is a capability name (`libc.so.6()(64bit)`,
//! `/usr/bin/perl`). A rich dependency, supported since rpm 4.13, is a
//! parenthesised expression such as `(python3-dbus if NetworkManager)` or
//! `(a or b)`. The leaf dependency tree (`classify_deps_rpm`) reads both.
//!
//! The tree is built from an installed host, so a rich dependency is
//! evaluated against what is installed there rather than solved: an `if`
//! condition holds when some installed package provides it, and an `or`
//! picks among the operands that are installed.

/// One dependency expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DepExpr {
    /// A capability name, version constraint stripped.
    Cap(String),
    /// `(a and b ...)`: every operand.
    And(Vec<DepExpr>),
    /// `(a or b ...)`: any operand.
    Or(Vec<DepExpr>),
    /// `(a with b ...)`: one package matching every operand. Treated as
    /// `and`, which credits the same providers when one package matches.
    With(Vec<DepExpr>),
    /// `(a without b)`: a package matching `a` and not `b`. Treated as `a`.
    Without(Box<DepExpr>),
    /// `(then if cond)` or `(then if cond else otherwise)`.
    If {
        then: Box<DepExpr>,
        cond: Box<DepExpr>,
        otherwise: Option<Box<DepExpr>>,
    },
    /// `(then unless cond)` or `(then unless cond else otherwise)`.
    Unless {
        then: Box<DepExpr>,
        cond: Box<DepExpr>,
        otherwise: Option<Box<DepExpr>>,
    },
}

/// How an `or` picks among its satisfied operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OrChoice {
    /// The first satisfied operand: a forward requirement needs only one,
    /// and on an installed host the first one present satisfies it.
    First,
    /// Every satisfied operand: in a reverse (Supplements) expression any
    /// one of them is enough to pull the package in.
    All,
}

impl DepExpr {
    /// Parse one dependency line as rpm prints it in `%{REQUIRENAME}` and
    /// the weak-dependency tags. Returns `None` for a rich expression this
    /// parser does not understand (unbalanced parentheses, operators mixed
    /// in one group), so the caller drops it as it did before rich
    /// dependencies were read at all.
    pub(super) fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.starts_with('(') {
            let mut parser = Parser {
                src: line.as_bytes(),
                pos: 0,
            };
            let expr = parser.group()?;
            parser.skip_ws();
            (parser.pos == parser.src.len()).then_some(expr)
        } else if line.is_empty() {
            None
        } else {
            // `%{REQUIRENAME}` and the weak tags hold the name alone, with
            // the version in a separate tag, so the whole line is the
            // capability. Splitting it would truncate a path with a space.
            Some(Self::Cap(line.to_string()))
        }
    }

    /// Every capability name in the expression, conditions included, so
    /// the caller can look all of them up in one pass over the index.
    pub(super) fn caps(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_caps(&mut out);
        out
    }

    fn collect_caps<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Self::Cap(c) => out.push(c),
            Self::And(xs) | Self::Or(xs) | Self::With(xs) => {
                xs.iter().for_each(|x| x.collect_caps(out));
            }
            Self::Without(x) => x.collect_caps(out),
            Self::If {
                then,
                cond,
                otherwise,
            }
            | Self::Unless {
                then,
                cond,
                otherwise,
            } => {
                then.collect_caps(out);
                cond.collect_caps(out);
                if let Some(o) = otherwise {
                    o.collect_caps(out);
                }
            }
        }
    }

    /// Whether the installed host satisfies the expression.
    pub(super) fn satisfied(&self, installed: &dyn Fn(&str) -> bool) -> bool {
        match self {
            Self::Cap(c) => installed(c),
            Self::And(xs) | Self::With(xs) => xs.iter().all(|x| x.satisfied(installed)),
            Self::Or(xs) => xs.iter().any(|x| x.satisfied(installed)),
            Self::Without(x) => x.satisfied(installed),
            Self::If {
                then,
                cond,
                otherwise,
            } => {
                if cond.satisfied(installed) {
                    then.satisfied(installed)
                } else {
                    otherwise.as_ref().is_none_or(|o| o.satisfied(installed))
                }
            }
            Self::Unless {
                then,
                cond,
                otherwise,
            } => {
                if cond.satisfied(installed) {
                    otherwise.as_ref().is_none_or(|o| o.satisfied(installed))
                } else {
                    then.satisfied(installed)
                }
            }
        }
    }

    /// The capabilities whose providers this expression pulls in, given
    /// what is installed. Conditions are evaluated, never credited: in
    /// `(a if b)` only `a` is pulled in, and only when `b` is installed.
    pub(super) fn pulled_caps(&self, installed: &dyn Fn(&str) -> bool, or: OrChoice) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_pulled(installed, or, &mut out);
        out
    }

    fn collect_pulled<'a>(
        &'a self,
        installed: &dyn Fn(&str) -> bool,
        or: OrChoice,
        out: &mut Vec<&'a str>,
    ) {
        match self {
            Self::Cap(c) => out.push(c),
            Self::And(xs) | Self::With(xs) => {
                xs.iter().for_each(|x| x.collect_pulled(installed, or, out));
            }
            Self::Or(xs) => {
                let mut satisfied = xs.iter().filter(|x| x.satisfied(installed));
                match or {
                    OrChoice::First => {
                        if let Some(x) = satisfied.next() {
                            x.collect_pulled(installed, or, out);
                        }
                    }
                    OrChoice::All => satisfied.for_each(|x| x.collect_pulled(installed, or, out)),
                }
            }
            Self::Without(x) => x.collect_pulled(installed, or, out),
            Self::If {
                then,
                cond,
                otherwise,
            } => {
                if cond.satisfied(installed) {
                    then.collect_pulled(installed, or, out);
                } else if let Some(o) = otherwise {
                    o.collect_pulled(installed, or, out);
                }
            }
            Self::Unless {
                then,
                cond,
                otherwise,
            } => {
                if !cond.satisfied(installed) {
                    then.collect_pulled(installed, or, out);
                } else if let Some(o) = otherwise {
                    o.collect_pulled(installed, or, out);
                }
            }
        }
    }
}

/// Byte-level recursive-descent parser for one rich expression.
///
/// Capability names may contain balanced parentheses
/// (`glibc-gconv-extra(aarch-64)`, `libc.so.6()(64bit)`), so a `(` opens a
/// nested group only where an operand starts, and inside a name a `)`
/// closes the enclosing group only when the name's own parentheses are
/// balanced.
struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.src.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// A word: runs to whitespace, or to a `)` that closes the enclosing
    /// group rather than one opened inside the word.
    fn word(&mut self) -> Option<&str> {
        let start = self.pos;
        let mut depth = 0usize;
        while let Some(b) = self.peek() {
            match b {
                b'(' => depth += 1,
                b')' if depth == 0 => break,
                b')' => depth -= 1,
                b if b.is_ascii_whitespace() => break,
                _ => {}
            }
            self.pos += 1;
        }
        (self.pos > start)
            .then(|| std::str::from_utf8(&self.src[start..self.pos]).ok())
            .flatten()
    }

    /// An operand: a nested group, or a capability with an optional
    /// version comparison (`foo >= 1.0`) that is read and dropped.
    fn operand(&mut self) -> Option<DepExpr> {
        self.skip_ws();
        if self.peek() == Some(b'(') {
            return self.group();
        }
        let name = self.word()?.to_string();
        let save = self.pos;
        self.skip_ws();
        let is_cmp = matches!(self.peek(), Some(b'<' | b'>' | b'='));
        if is_cmp {
            self.word()?; // the comparison operator
            self.skip_ws();
            self.word()?; // the version it compares against
        } else {
            self.pos = save;
        }
        Some(DepExpr::Cap(name))
    }

    /// A parenthesised group: operands joined by one kind of operator,
    /// except `if`/`unless` which take an optional `else`.
    fn group(&mut self) -> Option<DepExpr> {
        if self.peek() != Some(b'(') {
            return None;
        }
        self.pos += 1;
        let mut operands = vec![self.operand()?];
        let mut ops: Vec<String> = Vec::new();
        loop {
            self.skip_ws();
            match self.peek()? {
                b')' => {
                    self.pos += 1;
                    break;
                }
                _ => {
                    ops.push(self.word()?.to_string());
                    operands.push(self.operand()?);
                }
            }
        }
        build_group(operands, &ops)
    }
}

fn build_group(mut operands: Vec<DepExpr>, ops: &[String]) -> Option<DepExpr> {
    let ops: Vec<&str> = ops.iter().map(String::as_str).collect();
    match ops.as_slice() {
        [] => operands.pop(),
        ["if"] | ["unless"] | ["if", "else"] | ["unless", "else"] => {
            let otherwise = (operands.len() == 3)
                .then(|| Box::new(operands.pop().expect("an else group has three operands")));
            let cond = Box::new(operands.pop()?);
            let then = Box::new(operands.pop()?);
            Some(if ops[0] == "if" {
                DepExpr::If {
                    then,
                    cond,
                    otherwise,
                }
            } else {
                DepExpr::Unless {
                    then,
                    cond,
                    otherwise,
                }
            })
        }
        ["without"] => {
            operands.pop();
            Some(DepExpr::Without(Box::new(operands.pop()?)))
        }
        [first, rest @ ..] if rest.iter().all(|op| op == first) => match *first {
            "and" => Some(DepExpr::And(operands)),
            "or" => Some(DepExpr::Or(operands)),
            "with" => Some(DepExpr::With(operands)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(s: &str) -> DepExpr {
        DepExpr::Cap(s.to_string())
    }

    #[test]
    fn parses_plain_and_rich_forms() {
        // Forms measured in `rpm -qa` output on UBI 8 and UBI 9.
        let cases: &[(&str, Option<DepExpr>)] = &[
            ("libc.so.6()(64bit)", Some(cap("libc.so.6()(64bit)"))),
            ("/usr/bin/perl", Some(cap("/usr/bin/perl"))),
            (
                "(python3-dbus if NetworkManager)",
                Some(DepExpr::If {
                    then: Box::new(cap("python3-dbus")),
                    cond: Box::new(cap("NetworkManager")),
                    otherwise: None,
                }),
            ),
            (
                "(glibc-gconv-extra(aarch-64) = 2.34-284.el9_8.4 if redhat-rpm-config)",
                Some(DepExpr::If {
                    then: Box::new(cap("glibc-gconv-extra(aarch-64)")),
                    cond: Box::new(cap("redhat-rpm-config")),
                    otherwise: None,
                }),
            ),
            (
                "(python3.9dist(idna) < 3 with python3.9dist(idna) >= 2.5)",
                Some(DepExpr::With(vec![
                    cap("python3.9dist(idna)"),
                    cap("python3.9dist(idna)"),
                ])),
            ),
            (
                "(a or (b and c))",
                Some(DepExpr::Or(vec![
                    cap("a"),
                    DepExpr::And(vec![cap("b"), cap("c")]),
                ])),
            ),
            (
                "(a unless b else c)",
                Some(DepExpr::Unless {
                    then: Box::new(cap("a")),
                    cond: Box::new(cap("b")),
                    otherwise: Some(Box::new(cap("c"))),
                }),
            ),
            ("(a without b)", Some(DepExpr::Without(Box::new(cap("a"))))),
            // Mixed operators in one group are invalid in rpm too.
            ("(a and b or c)", None),
            ("(a and b", None),
            ("(a and b) trailing", None),
        ];
        for (input, expected) in cases {
            assert_eq!(&DepExpr::parse(input), expected, "input: {input}");
        }
    }

    #[test]
    fn pulled_caps_evaluates_conditions_against_installed() {
        let installed = |c: &str| matches!(c, "NetworkManager" | "b" | "c");
        let cases: &[(&str, OrChoice, &[&str])] = &[
            (
                "(python3-dbus if NetworkManager)",
                OrChoice::First,
                &["python3-dbus"],
            ),
            ("(grubby if kernel)", OrChoice::First, &[]),
            ("(a if kernel else c)", OrChoice::First, &["c"]),
            ("(a unless kernel)", OrChoice::First, &["a"]),
            ("(a unless b)", OrChoice::First, &[]),
            ("(a or b or c)", OrChoice::First, &["b"]),
            ("(a or b or c)", OrChoice::All, &["b", "c"]),
            ("(a or z)", OrChoice::First, &[]),
            ("(b and c)", OrChoice::First, &["b", "c"]),
        ];
        for (input, or, expected) in cases {
            let expr = DepExpr::parse(input).expect("parses");
            assert_eq!(
                &expr.pulled_caps(&installed, *or),
                expected,
                "input: {input}"
            );
        }
    }

    #[test]
    fn caps_lists_conditions_too() {
        let expr = DepExpr::parse("(a if b else (c or d))").expect("parses");
        assert_eq!(expr.caps(), vec!["a", "b", "c", "d"]);
    }
}
