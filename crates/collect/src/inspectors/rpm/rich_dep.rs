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
//!
//! A capability inside a rich expression keeps its version comparison
//! (`engine >= 2`), and only providers whose provided version satisfies it
//! count, compared as rpm compares them (`Constraint::overlaps`). A plain
//! requirement carries no version here: `%{REQUIREVERSION}` is not queried,
//! so it matches by name, as `rpm -q --whatprovides <name>` would.

use super::parser::rpmvercmp;
use std::cmp::Ordering;
use std::collections::HashSet;

/// How deeply rich expressions may nest. Real ones nest two or three
/// levels; rpm sets no limit, so a malformed or hostile line could nest
/// deeply enough to exhaust the stack in a recursive parser.
const MAX_RICH_DEP_DEPTH: usize = 16;

/// What dependency expressions are evaluated against: which installed
/// packages provide a capability.
pub(super) trait Providers {
    /// The installed packages, as canonical `name.arch` ids, that provide
    /// `atom` at a version it accepts (`Atom::accepts`). Empty when nothing
    /// installed does.
    fn providers(&self, atom: &Atom) -> HashSet<&str>;
}

/// A capability as a dependency names it, with its version comparison if
/// it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Atom {
    name: String,
    constraint: Option<Constraint>,
}

impl Atom {
    /// The capability name.
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    /// Whether a provide of this capability, at `provided` version, meets
    /// the atom's constraint. An unversioned atom accepts every provide and
    /// an unversioned provide meets every constraint, as in rpm.
    pub(super) fn accepts(&self, provided: Option<&Constraint>) -> bool {
        match (&self.constraint, provided) {
            (Some(wanted), Some(provided)) => provided.overlaps(wanted),
            _ => true,
        }
    }
}

/// A version comparison: an operator and the `[epoch:]version[-release]`
/// it compares against, on either side of a dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Constraint {
    cmp: Cmp,
    evr: String,
}

/// A dependency's comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmp {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

/// rpm's `RPMSENSE_LESS`, `RPMSENSE_GREATER` and `RPMSENSE_EQUAL` bits, as
/// `%{PROVIDEFLAGS}` prints them in decimal. Other bits in the flags mark
/// where a dependency came from, not how it compares.
const RPMSENSE_LESS: u32 = 1 << 1;
const RPMSENSE_GREATER: u32 = 1 << 2;
const RPMSENSE_EQUAL: u32 = 1 << 3;

impl Cmp {
    fn from_sense(less: bool, greater: bool, equal: bool) -> Option<Self> {
        match (less, greater, equal) {
            (true, false, false) => Some(Self::Lt),
            (true, false, true) => Some(Self::Le),
            (false, false, true) => Some(Self::Eq),
            (false, true, true) => Some(Self::Ge),
            (false, true, false) => Some(Self::Gt),
            _ => None,
        }
    }

    /// An operator as written in a rich dependency (`<`, `<=`, `=`, `==`,
    /// `>=`, `>`).
    fn from_operator(op: &str) -> Option<Self> {
        if op.is_empty() || !op.bytes().all(|b| matches!(b, b'<' | b'>' | b'=')) {
            return None;
        }
        Self::from_sense(op.contains('<'), op.contains('>'), op.contains('='))
    }

    fn less(self) -> bool {
        matches!(self, Self::Lt | Self::Le)
    }

    fn greater(self) -> bool {
        matches!(self, Self::Gt | Self::Ge)
    }

    fn equal(self) -> bool {
        matches!(self, Self::Le | Self::Eq | Self::Ge)
    }
}

impl Constraint {
    /// A provide's version from the provides index: `%{PROVIDEFLAGS}` and
    /// `%{PROVIDEVERSION}`. `None` for an unversioned provide, which rpm
    /// treats as meeting any constraint.
    pub(super) fn from_provide(flags: u32, evr: &str) -> Option<Self> {
        let cmp = Cmp::from_sense(
            flags & RPMSENSE_LESS != 0,
            flags & RPMSENSE_GREATER != 0,
            flags & RPMSENSE_EQUAL != 0,
        )?;
        (!evr.is_empty()).then(|| Self {
            cmp,
            evr: evr.to_string(),
        })
    }

    /// Whether the two version ranges overlap, by rpm's rule
    /// (`rpmdsCompare`): compare the two EVRs, then check that the
    /// operators leave some version in common.
    fn overlaps(&self, other: &Self) -> bool {
        let (a, b) = (self.cmp, other.cmp);
        match evr_cmp(&self.evr, &other.evr) {
            Ordering::Less => a.greater() || b.less(),
            Ordering::Greater => a.less() || b.greater(),
            Ordering::Equal => {
                (a.equal() && b.equal()) || (a.less() && b.less()) || (a.greater() && b.greater())
            }
        }
    }
}

/// Split `[epoch:]version[-release]` as rpm does (`rpmverParse`): the
/// epoch is a leading run of digits before `:`, the release follows the
/// last `-`.
fn split_evr(evr: &str) -> (&str, &str, Option<&str>) {
    let digits = evr.bytes().take_while(u8::is_ascii_digit).count();
    let (epoch, rest) = match evr[digits..].strip_prefix(':') {
        Some(rest) => (&evr[..digits], rest),
        None => ("", evr),
    };
    let (version, release) = match rest.rsplit_once('-') {
        Some((v, r)) => (v, Some(r)),
        None => (rest, None),
    };
    (if epoch.is_empty() { "0" } else { epoch }, version, release)
}

/// Compare two EVRs as rpm does (`rpmverCmp`): a missing epoch is 0, and
/// the release counts only when both sides have one, so `foo >= 2.0`
/// accepts `2.0-1`.
fn evr_cmp(a: &str, b: &str) -> Ordering {
    let (ea, va, ra) = split_evr(a);
    let (eb, vb, rb) = split_evr(b);
    rpmvercmp(ea, eb)
        .then_with(|| rpmvercmp(va, vb))
        .then_with(|| match (ra, rb) {
            (Some(ra), Some(rb)) if !ra.is_empty() && !rb.is_empty() => rpmvercmp(ra, rb),
            _ => Ordering::Equal,
        })
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
    /// A capability, with its version comparison if it has one.
    Cap(Atom),
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
            Node::Match(Match::Cap(Atom {
                name: line.to_string(),
                constraint: None,
            }))
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
    /// pulled in, and only when `b` is installed. A package that itself
    /// makes the condition hold is not pulled in by it either, so the
    /// version floor `(foo >= 2 if foo)` pulls in nothing: it constrains a
    /// `foo` that is already there.
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
                    let mut pulled = HashSet::new();
                    then.collect_pulled(index, or, &mut pulled);
                    out.extend(&pulled - &cond.witnesses(index));
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

    /// The installed packages that make this node hold when it is a
    /// condition: the providers of every capability it matches on.
    fn witnesses<'p>(&self, index: &'p dyn Providers) -> HashSet<&'p str> {
        match self {
            Self::Match(m) => m.matching(index),
            Self::And(xs) | Self::Or(xs) => xs.iter().flat_map(|x| x.witnesses(index)).collect(),
            Self::If {
                then, otherwise, ..
            }
            | Self::Unless {
                then, otherwise, ..
            } => then
                .witnesses(index)
                .into_iter()
                .chain(otherwise.iter().flat_map(|o| o.witnesses(index)))
                .collect(),
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
            Self::Cap(atom) => out.push(&atom.name),
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
            Self::Cap(atom) => index.providers(atom),
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
    /// version comparison (`foo >= 1.0`).
    fn operand(&mut self) -> Option<Node> {
        self.skip_ws();
        if self.peek() == Some(b'(') {
            return self.group();
        }
        let name = self.word()?.to_string();
        let save = self.pos;
        self.skip_ws();
        let constraint = if matches!(self.peek(), Some(b'<' | b'>' | b'=')) {
            let cmp = Cmp::from_operator(self.word()?)?;
            self.skip_ws();
            let evr = self.word()?.to_string();
            Some(Constraint { cmp, evr })
        } else {
            self.pos = save;
            None
        };
        Some(Node::Match(Match::Cap(Atom { name, constraint })))
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
        Match::Cap(Atom {
            name: s.to_string(),
            constraint: None,
        })
    }

    fn mcap_v(s: &str, cmp: Cmp, evr: &str) -> Match {
        Match::Cap(Atom {
            name: s.to_string(),
            constraint: Some(Constraint {
                cmp,
                evr: evr.to_string(),
            }),
        })
    }

    fn cap(s: &str) -> Node {
        Node::Match(mcap(s))
    }

    /// An installed host as (package id, capability it provides) pairs.
    /// A capability written `name = evr` is a versioned provide.
    struct TestIndex(&'static [(&'static str, &'static str)]);

    impl Providers for TestIndex {
        fn providers(&self, atom: &Atom) -> HashSet<&str> {
            self.0
                .iter()
                .filter(|(_, c)| {
                    let (name, provided) = match c.split_once(" = ") {
                        Some((name, evr)) => (
                            name,
                            Some(Constraint {
                                cmp: Cmp::Eq,
                                evr: evr.to_string(),
                            }),
                        ),
                        None => (*c, None),
                    };
                    name == atom.name() && atom.accepts(provided.as_ref())
                })
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
                    then: Box::new(Node::Match(mcap_v(
                        "glibc-gconv-extra(aarch-64)",
                        Cmp::Eq,
                        "2.34-284.el9_8.4",
                    ))),
                    cond: Box::new(cap("redhat-rpm-config")),
                    otherwise: None,
                }),
            ),
            (
                "(python3.9dist(idna) < 3 with python3.9dist(idna) >= 2.5)",
                Some(Node::Match(Match::With(vec![
                    mcap_v("python3.9dist(idna)", Cmp::Lt, "3"),
                    mcap_v("python3.9dist(idna)", Cmp::Ge, "2.5"),
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
            // `!=` is not an rpm operator.
            ("(a != 1 if b)", None),
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
    fn versioned_conditions_and_alternatives_check_the_provided_version() {
        // Only `engine = 1` is installed, so `engine >= 2` does not hold.
        // `foo >= 2 if foo` is a version floor on an installed foo, not a
        // reason to install foo; it holds or fails, and pulls in nothing.
        let index = TestIndex(&[
            ("engine", "engine = 1-1"),
            ("plugin", "plugin = 2-1"),
            ("fallback", "fallback = 1-1"),
            ("foo", "foo = 2.1-3"),
            ("foo-devel", "foo-devel = 2.1-3"),
        ]);
        let cases: &[(&str, &[&str])] = &[
            ("(plugin if engine >= 2 else fallback)", &["fallback"]),
            ("(plugin if engine >= 1 else fallback)", &["plugin"]),
            ("(plugin if engine < 2)", &["plugin"]),
            ("(plugin >= 3 or fallback)", &["fallback"]),
            ("(plugin >= 2 or fallback)", &["plugin"]),
            ("(foo >= 2 if foo)", &[]),
            ("(foo-devel = 2.1-3 if foo)", &["foo-devel"]),
            ("(foo-devel = 2.1-4 if foo)", &[]),
        ];
        for (input, expected) in cases {
            let expr = DepExpr::parse(input).expect("parses");
            assert_eq!(
                expr.pulled(&index, OrChoice::First),
                set(expected),
                "input: {input}"
            );
        }
    }

    #[test]
    fn constraints_overlap_as_rpm_compares_them() {
        let c = |cmp, evr: &str| Constraint {
            cmp,
            evr: evr.to_string(),
        };
        // (provide, requirement, overlaps)
        let cases = [
            (c(Cmp::Eq, "1-1"), c(Cmp::Ge, "2"), false),
            (c(Cmp::Eq, "2.0-1"), c(Cmp::Ge, "2.0"), true),
            (c(Cmp::Eq, "2.0-1"), c(Cmp::Eq, "2.0-2"), false),
            (c(Cmp::Eq, "2.0-1"), c(Cmp::Lt, "2.0-2"), true),
            (c(Cmp::Eq, "1:1.0-1"), c(Cmp::Ge, "2.0"), true),
            (c(Cmp::Eq, "0:2.0-1"), c(Cmp::Eq, "2.0-1"), true),
            (c(Cmp::Eq, "1.0~rc1-1"), c(Cmp::Ge, "1.0"), false),
            (c(Cmp::Ge, "3"), c(Cmp::Lt, "2"), false),
            (c(Cmp::Ge, "3"), c(Cmp::Gt, "2"), true),
        ];
        for (provided, wanted, expected) in cases {
            assert_eq!(
                provided.overlaps(&wanted),
                expected,
                "{provided:?} against {wanted:?}"
            );
        }
        // An unversioned provide meets any constraint.
        assert_eq!(Constraint::from_provide(0, ""), None);
        assert_eq!(
            Constraint::from_provide(RPMSENSE_EQUAL | (1 << 15), "2-1"),
            Some(c(Cmp::Eq, "2-1"))
        );
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
