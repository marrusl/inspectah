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
//! picks among the operands that are installed. Evaluation works on sets
//! of installed packages, not on capability names, because `with` and
//! `without` are defined per package: `(a with b)` needs one package that
//! provides both.

use std::collections::HashSet;

/// How deeply rich expressions may nest. Real ones nest two or three
/// levels; rpm sets no limit, so a malformed or hostile line could nest
/// deeply enough to exhaust the stack in a recursive parser.
const MAX_RICH_DEP_DEPTH: usize = 16;

/// What dependency expressions are evaluated against: which installed
/// packages provide a capability.
pub(super) trait Providers {
    /// The installed packages, as canonical `name.arch` ids, that provide
    /// `cap`. Empty when nothing installed provides it.
    fn providers(&self, cap: &str) -> HashSet<&str>;
}

/// One parsed dependency expression.
///
/// Its nesting depth is at most `MAX_RICH_DEP_DEPTH`. The parser is the
/// only constructor and enforces that, so every recursive walk over the
/// tree (`caps`, `satisfied`, `pulled`) is bounded by the same limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DepExpr {
    root: Node,
}

/// One node of a dependency expression.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    /// Packages selected one at a time: a capability, `with` or `without`.
    Match(Match),
    /// `(a and b ...)`: every operand, each from any package.
    And(Vec<Node>),
    /// `(a or b ...)`: any operand.
    Or(Vec<Node>),
    /// `(then if cond)` or `(then if cond else otherwise)`.
    If {
        then: Box<Node>,
        cond: Box<Node>,
        otherwise: Option<Box<Node>>,
    },
    /// `(then unless cond)` or `(then unless cond else otherwise)`.
    Unless {
        then: Box<Node>,
        cond: Box<Node>,
        otherwise: Option<Box<Node>>,
    },
}

/// A selector of single packages, judged one package at a time.
///
/// rpm defines `with` and `without` as intersection and subtraction over
/// the packages that match each operand, so their operands must select
/// packages too: a capability, another `with` or `without`, or an `or` of
/// those. `and`, `if` and `unless` say nothing about one package and are
/// rejected there by the parser.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Match {
    /// A capability name, version constraint stripped.
    Cap(String),
    /// `(a with b ...)`: packages that match every operand.
    With(Vec<Match>),
    /// `(a without b)`: packages that match `a` and not `b`.
    Without(Box<Match>, Box<Match>),
    /// `(a or b ...)` inside `with` or `without`: packages that match any
    /// operand.
    Any(Vec<Match>),
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
    /// in one group, an `and`, `if` or `unless` under `with` or `without`,
    /// nesting deeper than `MAX_RICH_DEP_DEPTH`), so the caller drops it as
    /// it did before rich dependencies were read at all.
    pub(super) fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        let root = if line.starts_with('(') {
            let mut parser = Parser {
                src: line.as_bytes(),
                pos: 0,
                depth: 0,
            };
            let expr = parser.group()?;
            parser.skip_ws();
            (parser.pos == parser.src.len()).then_some(expr)?
        } else if line.is_empty() {
            return None;
        } else {
            // `%{REQUIRENAME}` and the weak tags hold the name alone, with
            // the version in a separate tag, so the whole line is the
            // capability. Splitting it would truncate a path with a space.
            Node::Match(Match::Cap(line.to_string()))
        };
        Some(Self { root })
    }

    /// Every capability name in the expression, conditions included, so
    /// the caller can look all of them up in one pass over the index.
    pub(super) fn caps(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.root.collect_caps(&mut out);
        out
    }

    /// Whether the installed host satisfies the expression.
    pub(super) fn satisfied(&self, index: &dyn Providers) -> bool {
        self.root.satisfied(index)
    }

    /// The installed packages this expression pulls in. Conditions are
    /// evaluated, never credited: in `(a if b)` only a provider of `a` is
    /// pulled in, and only when `b` is installed.
    pub(super) fn pulled<'p>(&self, index: &'p dyn Providers, or: OrChoice) -> HashSet<&'p str> {
        let mut out = HashSet::new();
        self.root.collect_pulled(index, or, &mut out);
        out
    }
}

impl Node {
    fn collect_caps<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Self::Match(m) => m.collect_caps(out),
            Self::And(xs) | Self::Or(xs) => {
                xs.iter().for_each(|x| x.collect_caps(out));
            }
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

    fn satisfied(&self, index: &dyn Providers) -> bool {
        match self {
            Self::Match(m) => !m.matching(index).is_empty(),
            Self::And(xs) => xs.iter().all(|x| x.satisfied(index)),
            Self::Or(xs) => xs.iter().any(|x| x.satisfied(index)),
            Self::If {
                then,
                cond,
                otherwise,
            } => {
                if cond.satisfied(index) {
                    then.satisfied(index)
                } else {
                    otherwise.as_ref().is_none_or(|o| o.satisfied(index))
                }
            }
            Self::Unless {
                then,
                cond,
                otherwise,
            } => {
                if cond.satisfied(index) {
                    otherwise.as_ref().is_none_or(|o| o.satisfied(index))
                } else {
                    then.satisfied(index)
                }
            }
        }
    }

    fn collect_pulled<'p>(
        &self,
        index: &'p dyn Providers,
        or: OrChoice,
        out: &mut HashSet<&'p str>,
    ) {
        match self {
            Self::Match(m) => out.extend(m.matching(index)),
            Self::And(xs) => {
                xs.iter().for_each(|x| x.collect_pulled(index, or, out));
            }
            Self::Or(xs) => {
                let mut satisfied = xs.iter().filter(|x| x.satisfied(index));
                match or {
                    OrChoice::First => {
                        if let Some(x) = satisfied.next() {
                            x.collect_pulled(index, or, out);
                        }
                    }
                    OrChoice::All => satisfied.for_each(|x| x.collect_pulled(index, or, out)),
                }
            }
            Self::If {
                then,
                cond,
                otherwise,
            } => {
                if cond.satisfied(index) {
                    then.collect_pulled(index, or, out);
                } else if let Some(o) = otherwise {
                    o.collect_pulled(index, or, out);
                }
            }
            Self::Unless {
                then,
                cond,
                otherwise,
            } => {
                if !cond.satisfied(index) {
                    then.collect_pulled(index, or, out);
                } else if let Some(o) = otherwise {
                    o.collect_pulled(index, or, out);
                }
            }
        }
    }

    /// The package selector this node stands for inside `with` or
    /// `without`, or `None` when it does not select single packages.
    fn into_match(self) -> Option<Match> {
        match self {
            Self::Match(m) => Some(m),
            Self::Or(xs) => xs
                .into_iter()
                .map(Self::into_match)
                .collect::<Option<Vec<_>>>()
                .map(Match::Any),
            Self::And(_) | Self::If { .. } | Self::Unless { .. } => None,
        }
    }
}

impl Match {
    fn collect_caps<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Self::Cap(c) => out.push(c),
            Self::With(xs) | Self::Any(xs) => xs.iter().for_each(|x| x.collect_caps(out)),
            Self::Without(a, b) => {
                a.collect_caps(out);
                b.collect_caps(out);
            }
        }
    }

    /// The installed packages this selector matches.
    fn matching<'p>(&self, index: &'p dyn Providers) -> HashSet<&'p str> {
        match self {
            Self::Cap(c) => index.providers(c),
            Self::With(xs) => {
                let mut sets = xs.iter().map(|x| x.matching(index));
                let first = sets.next().unwrap_or_default();
                sets.fold(first, |acc, s| &acc & &s)
            }
            Self::Without(a, b) => &a.matching(index) - &b.matching(index),
            Self::Any(xs) => xs.iter().flat_map(|x| x.matching(index)).collect(),
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
    /// Groups currently open, checked against `MAX_RICH_DEP_DEPTH`.
    depth: usize,
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
    fn operand(&mut self) -> Option<Node> {
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
        Some(Node::Match(Match::Cap(name)))
    }

    /// A parenthesised group: operands joined by one kind of operator,
    /// except `if`/`unless` which take an optional `else`.
    fn group(&mut self) -> Option<Node> {
        if self.peek() != Some(b'(') || self.depth == MAX_RICH_DEP_DEPTH {
            return None;
        }
        self.pos += 1;
        self.depth += 1;
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
        self.depth -= 1;
        build_group(operands, &ops)
    }
}

fn build_group(mut operands: Vec<Node>, ops: &[String]) -> Option<Node> {
    let ops: Vec<&str> = ops.iter().map(String::as_str).collect();
    match ops.as_slice() {
        [] => operands.pop(),
        ["if"] | ["unless"] | ["if", "else"] | ["unless", "else"] => {
            let otherwise = (operands.len() == 3)
                .then(|| Box::new(operands.pop().expect("an else group has three operands")));
            let cond = Box::new(operands.pop()?);
            let then = Box::new(operands.pop()?);
            Some(if ops[0] == "if" {
                Node::If {
                    then,
                    cond,
                    otherwise,
                }
            } else {
                Node::Unless {
                    then,
                    cond,
                    otherwise,
                }
            })
        }
        ["without"] => {
            let exclude = operands.pop()?.into_match()?;
            let include = operands.pop()?.into_match()?;
            Some(Node::Match(Match::Without(
                Box::new(include),
                Box::new(exclude),
            )))
        }
        [first, rest @ ..] if rest.iter().all(|op| op == first) => match *first {
            "and" => Some(Node::And(operands)),
            "or" => Some(Node::Or(operands)),
            "with" => operands
                .into_iter()
                .map(Node::into_match)
                .collect::<Option<Vec<_>>>()
                .map(|xs| Node::Match(Match::With(xs))),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mcap(s: &str) -> Match {
        Match::Cap(s.to_string())
    }

    fn cap(s: &str) -> Node {
        Node::Match(mcap(s))
    }

    /// An installed host as (package id, capability it provides) pairs.
    struct TestIndex(&'static [(&'static str, &'static str)]);

    impl Providers for TestIndex {
        fn providers(&self, cap: &str) -> HashSet<&str> {
            self.0
                .iter()
                .filter(|(_, c)| *c == cap)
                .map(|(id, _)| *id)
                .collect()
        }
    }

    fn set<'a>(ids: &[&'a str]) -> HashSet<&'a str> {
        ids.iter().copied().collect()
    }

    #[test]
    fn parses_plain_and_rich_forms() {
        // Forms measured in `rpm -qa` output on UBI 8 and UBI 9.
        let cases: &[(&str, Option<Node>)] = &[
            ("libc.so.6()(64bit)", Some(cap("libc.so.6()(64bit)"))),
            ("/usr/bin/perl", Some(cap("/usr/bin/perl"))),
            (
                "(python3-dbus if NetworkManager)",
                Some(Node::If {
                    then: Box::new(cap("python3-dbus")),
                    cond: Box::new(cap("NetworkManager")),
                    otherwise: None,
                }),
            ),
            (
                "(glibc-gconv-extra(aarch-64) = 2.34-284.el9_8.4 if redhat-rpm-config)",
                Some(Node::If {
                    then: Box::new(cap("glibc-gconv-extra(aarch-64)")),
                    cond: Box::new(cap("redhat-rpm-config")),
                    otherwise: None,
                }),
            ),
            (
                "(python3.9dist(idna) < 3 with python3.9dist(idna) >= 2.5)",
                Some(Node::Match(Match::With(vec![
                    mcap("python3.9dist(idna)"),
                    mcap("python3.9dist(idna)"),
                ]))),
            ),
            (
                "(a or (b and c))",
                Some(Node::Or(vec![
                    cap("a"),
                    Node::And(vec![cap("b"), cap("c")]),
                ])),
            ),
            (
                "(a unless b else c)",
                Some(Node::Unless {
                    then: Box::new(cap("a")),
                    cond: Box::new(cap("b")),
                    otherwise: Some(Box::new(cap("c"))),
                }),
            ),
            (
                "(a without b)",
                Some(Node::Match(Match::Without(
                    Box::new(mcap("a")),
                    Box::new(mcap("b")),
                ))),
            ),
            (
                "(a with (b or c))",
                Some(Node::Match(Match::With(vec![
                    mcap("a"),
                    Match::Any(vec![mcap("b"), mcap("c")]),
                ]))),
            ),
            // Mixed operators in one group are invalid in rpm too.
            ("(a and b or c)", None),
            ("(a and b", None),
            ("(a and b) trailing", None),
            // `with` and `without` select single packages; `and` and `if`
            // do not.
            ("(a with (b and c))", None),
            ("(a without (b if c))", None),
        ];
        for (input, expected) in cases {
            assert_eq!(
                &DepExpr::parse(input).map(|e| e.root),
                expected,
                "input: {input}"
            );
        }
    }

    #[test]
    fn pulled_evaluates_conditions_against_installed() {
        let index = TestIndex(&[
            ("NetworkManager", "NetworkManager"),
            ("a", "a"),
            ("b", "b"),
            ("c", "c"),
            ("grubby", "grubby"),
            ("python3-dbus", "python3-dbus"),
        ]);
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
            ("(z or b or c)", OrChoice::First, &["b"]),
            ("(z or b or c)", OrChoice::All, &["b", "c"]),
            ("(z or y)", OrChoice::First, &[]),
            ("(b and c)", OrChoice::First, &["b", "c"]),
        ];
        for (input, or, expected) in cases {
            let expr = DepExpr::parse(input).expect("parses");
            assert_eq!(expr.pulled(&index, *or), set(expected), "input: {input}");
        }
    }

    #[test]
    fn with_and_without_select_single_packages() {
        // P provides only `a`, Q only `b`, R both. rpm defines `with` as
        // one package matching every operand and `without` as a package
        // matching the left operand and not the right.
        let index = TestIndex(&[("P", "a"), ("Q", "b"), ("R", "a"), ("R", "b"), ("S", "c")]);
        let cases: &[(&str, &[&str])] = &[
            ("(a with b)", &["R"]),
            ("(a without b)", &["P"]),
            ("(b without a)", &["Q"]),
            ("(a with c)", &[]),
            ("(a with (b or c))", &["R"]),
            ("((a with b) or c)", &["R"]),
        ];
        for (input, expected) in cases {
            let expr = DepExpr::parse(input).expect("parses");
            assert_eq!(
                expr.pulled(&index, OrChoice::First),
                set(expected),
                "input: {input}"
            );
            assert_eq!(
                expr.satisfied(&index),
                !expected.is_empty(),
                "input: {input}"
            );
        }
    }

    #[test]
    fn deep_nesting_is_rejected_without_overflowing_the_stack() {
        let deep = "(".repeat(200_000);
        assert_eq!(DepExpr::parse(&deep), None);
        // The limit itself parses; one more level does not.
        let nested = |n: usize| format!("{}a{}", "(".repeat(n), ")".repeat(n));
        assert!(DepExpr::parse(&nested(MAX_RICH_DEP_DEPTH)).is_some());
        assert_eq!(DepExpr::parse(&nested(MAX_RICH_DEP_DEPTH + 1)), None);
    }

    #[test]
    fn caps_lists_conditions_too() {
        let expr = DepExpr::parse("(a if b else (c or d))").expect("parses");
        assert_eq!(expr.caps(), vec!["a", "b", "c", "d"]);
        let expr = DepExpr::parse("(a without b)").expect("parses");
        assert_eq!(expr.caps(), vec!["a", "b"]);
    }
}
