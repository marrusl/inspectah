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

/// How many alternative trigger sets a Supplements expression may expand
/// to (`DepExpr::supplement_triggers`). An `and` of `or`s multiplies out;
/// real expressions expand to a handful, and one that expands past this is
/// dropped rather than grown without bound.
const MAX_SUPPLEMENT_CLAUSES: usize = 64;

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
    /// (`rpmdsCompare`, which calls `rpmverOverlap`): compare the two EVRs,
    /// then check that the operators leave some version in common. A
    /// missing epoch is 0, and the release counts only when both sides
    /// have one, so `foo >= 2.0` accepts `2.0-1`.
    fn overlaps(&self, other: &Self) -> bool {
        let (a, b) = (self.cmp, other.cmp);
        let (ea, va, ra) = split_evr(&self.evr);
        let (eb, vb, rb) = split_evr(&other.evr);
        let sense = match rpmvercmp(ea, eb).then_with(|| rpmvercmp(va, vb)) {
            Ordering::Equal => match (ra, rb) {
                (Some(ra), Some(rb)) => rpmvercmp(ra, rb),
                // Epoch and version are equal and at most one side has a
                // release. rpm then counts a match outright when the side
                // without a release has EQUAL, before looking at the other
                // side's operator, so `engine = 2` meets `engine > 2-1`.
                // `rpmverOverlap` in rpmio/rpmver.c, lines 74-85, the same
                // in rpm-4.16.1-release and rpm-4.19.1.1-release:
                //   } else {
                //     /* always matches if the side with no release has SENSE_EQUAL */
                //     if ((v1->r && *v1->r && (f2 & RPMSENSE_EQUAL)) ||
                //         (v2->r && *v2->r && (f1 & RPMSENSE_EQUAL))) {
                //       result = 1;
                (Some(_), None) if b.equal() => return true,
                (None, Some(_)) if a.equal() => return true,
                _ => Ordering::Equal,
            },
            sense => sense,
        };
        match sense {
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
/// last `-`. A missing epoch reads as `0`, and an empty release as none,
/// as `rpmverOverlap` tests `v->r && *v->r`.
fn split_evr(evr: &str) -> (&str, &str, Option<&str>) {
    let digits = evr.bytes().take_while(u8::is_ascii_digit).count();
    let (epoch, rest) = match evr[digits..].strip_prefix(':') {
        Some(rest) => (&evr[..digits], rest),
        None => ("", evr),
    };
    let (version, release) = match rest.rsplit_once('-') {
        Some((v, r)) => (v, Some(r).filter(|r| !r.is_empty())),
        None => (rest, None),
    };
    (if epoch.is_empty() { "0" } else { epoch }, version, release)
}

/// One parsed dependency expression.
///
/// Its nesting depth is at most `MAX_RICH_DEP_DEPTH`. The parser is the
/// only constructor and enforces that, so every recursive walk over the
/// tree (`caps`, `pulled`, `supplement_triggers`) is bounded by the same limit.
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

/// When a Supplements expression pulls its package in, in terms of the
/// added packages: those not in the base image.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SupplementTriggers<'p> {
    /// The expression holds through packages that are not added, before
    /// any added package is installed. dnf does not pull a package in for
    /// a Supplements that already held, so no added package triggers it.
    AlreadyHolds,
    /// Alternative clauses. The package is pulled in once every group in
    /// some clause has a member present; a group is the added packages
    /// any one of which satisfies one operand. No clause is empty.
    Clauses(Vec<Vec<HashSet<&'p str>>>),
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

    /// The installed packages this expression pulls in. Conditions are
    /// evaluated, never credited: in `(a if b)` only a provider of `a` is
    /// pulled in, and only when `b` is installed. A package that itself
    /// makes the condition hold is not pulled in by it either, so the
    /// version floor `(foo >= 2 if foo)` pulls in nothing: it constrains a
    /// `foo` that is already there. An `or` takes its first satisfied
    /// operand: a requirement needs only one, and on an installed host the
    /// first one present satisfies it.
    pub(super) fn pulled<'p>(&self, index: &'p dyn Providers) -> HashSet<&'p str> {
        let mut out = HashSet::new();
        self.root.collect_pulled(index, &mut out);
        out
    }

    /// Read the expression as a Supplements: which added packages, present
    /// together, pull the supplementing package in. `(a and b)` needs a
    /// provider of each, so it is one clause of two groups, never two
    /// independent triggers; `(a or b)` is two clauses. An operand that a
    /// non-added package satisfies is already true and adds no group.
    /// `if` and `unless` conditions are judged on the host, and a condition
    /// that holds through added packages joins the clause as an operand.
    ///
    /// Returns `None` when the expression expands to more than
    /// `MAX_SUPPLEMENT_CLAUSES` clauses.
    pub(super) fn supplement_triggers<'p>(
        &self,
        index: &'p dyn Providers,
        is_added: &dyn Fn(&str) -> bool,
    ) -> Option<SupplementTriggers<'p>> {
        let clauses = self.root.trigger_clauses(index, is_added)?;
        Some(if clauses.iter().any(Vec::is_empty) {
            SupplementTriggers::AlreadyHolds
        } else {
            SupplementTriggers::Clauses(clauses)
        })
    }
}

/// One alternative of `SupplementTriggers::Clauses`.
type Clause<'p> = Vec<HashSet<&'p str>>;

/// Every clause of `a` joined with every clause of `b`: the triggers of
/// `a and b`. `None` past `MAX_SUPPLEMENT_CLAUSES`.
fn conjoin<'p>(a: Vec<Clause<'p>>, b: &[Clause<'p>]) -> Option<Vec<Clause<'p>>> {
    if a.len().saturating_mul(b.len()) > MAX_SUPPLEMENT_CLAUSES {
        return None;
    }
    Some(
        a.iter()
            .flat_map(|x| b.iter().map(move |y| x.iter().chain(y).cloned().collect()))
            .collect(),
    )
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

    fn collect_pulled<'p>(&self, index: &'p dyn Providers, out: &mut HashSet<&'p str>) {
        match self {
            Self::Match(m) => out.extend(m.matching(index)),
            Self::And(xs) => {
                xs.iter().for_each(|x| x.collect_pulled(index, out));
            }
            Self::Or(xs) => {
                if let Some(x) = xs.iter().find(|x| x.satisfied(index)) {
                    x.collect_pulled(index, out);
                }
            }
            Self::If {
                then,
                cond,
                otherwise,
            } => {
                if cond.satisfied(index) {
                    let mut pulled = HashSet::new();
                    then.collect_pulled(index, &mut pulled);
                    out.extend(&pulled - &cond.witnesses(index));
                } else if let Some(o) = otherwise {
                    o.collect_pulled(index, out);
                }
            }
            Self::Unless {
                then,
                cond,
                otherwise,
            } => {
                if !cond.satisfied(index) {
                    then.collect_pulled(index, out);
                } else if let Some(o) = otherwise {
                    o.collect_pulled(index, out);
                }
            }
        }
    }

    /// This node's triggers as part of a Supplements (`supplement_triggers`).
    fn trigger_clauses<'p>(
        &self,
        index: &'p dyn Providers,
        is_added: &dyn Fn(&str) -> bool,
    ) -> Option<Vec<Clause<'p>>> {
        match self {
            Self::Match(m) => {
                let ids = m.matching(index);
                Some(if ids.is_empty() {
                    Vec::new()
                } else if ids.iter().any(|id| !is_added(id)) {
                    vec![Vec::new()]
                } else {
                    vec![vec![ids]]
                })
            }
            Self::And(xs) => xs.iter().try_fold(vec![Vec::new()], |acc, x| {
                conjoin(acc, &x.trigger_clauses(index, is_added)?)
            }),
            Self::Or(xs) => {
                let mut out = Vec::new();
                for x in xs {
                    out.extend(x.trigger_clauses(index, is_added)?);
                    if out.len() > MAX_SUPPLEMENT_CLAUSES {
                        return None;
                    }
                }
                Some(out)
            }
            Self::If {
                then,
                cond,
                otherwise,
            } => {
                let cond_triggers = cond.trigger_clauses(index, is_added)?;
                if cond.satisfied(index) {
                    conjoin(then.trigger_clauses(index, is_added)?, &cond_triggers)
                } else {
                    otherwise
                        .as_ref()
                        .map_or(Some(Vec::new()), |o| o.trigger_clauses(index, is_added))
                }
            }
            Self::Unless {
                then,
                cond,
                otherwise,
            } => {
                if cond.satisfied(index) {
                    let cond_triggers = cond.trigger_clauses(index, is_added)?;
                    let otherwise = otherwise
                        .as_ref()
                        .map_or(Some(Vec::new()), |o| o.trigger_clauses(index, is_added))?;
                    conjoin(otherwise, &cond_triggers)
                } else {
                    then.trigger_clauses(index, is_added)
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
        let cases: &[(&str, &[&str])] = &[
            ("(python3-dbus if NetworkManager)", &["python3-dbus"]),
            ("(grubby if kernel)", &[]),
            ("(a if kernel else c)", &["c"]),
            ("(a unless kernel)", &["a"]),
            ("(a unless b)", &[]),
            ("(z or b or c)", &["b"]),
            ("(z or y)", &[]),
            ("(b and c)", &["b", "c"]),
        ];
        for (input, expected) in cases {
            let expr = DepExpr::parse(input).expect("parses");
            assert_eq!(expr.pulled(&index), set(expected), "input: {input}");
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
            assert_eq!(expr.pulled(&index), set(expected), "input: {input}");
            assert_eq!(
                expr.root.satisfied(&index),
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
            assert_eq!(expr.pulled(&index), set(expected), "input: {input}");
        }
    }

    #[test]
    fn a_release_less_equal_provide_meets_a_strict_comparison_on_its_version() {
        // `engine = 2` carries no release. Against `engine > 2-1` rpm
        // compares epoch and version only, finds them equal, and counts
        // the match because the release-less side has EQUAL.
        let index = TestIndex(&[
            ("engine", "engine = 2"),
            ("plugin", "plugin"),
            ("fallback", "fallback"),
        ]);
        let cases: &[(&str, &[&str])] = &[
            ("(plugin if engine > 2-1 else fallback)", &["plugin"]),
            ("(plugin if engine < 2-1 else fallback)", &["plugin"]),
            ("(plugin if engine > 2 else fallback)", &["fallback"]),
            ("(plugin if engine > 1.9-1 else fallback)", &["plugin"]),
        ];
        for (input, expected) in cases {
            let expr = DepExpr::parse(input).expect("parses");
            assert_eq!(expr.pulled(&index), set(expected), "input: {input}");
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
            // Epoch and version equal, one side without a release: rpm
            // counts a match when that side has EQUAL, whatever the other
            // side's operator (`rpmverOverlap`).
            (c(Cmp::Eq, "2"), c(Cmp::Gt, "2-1"), true),
            (c(Cmp::Eq, "2"), c(Cmp::Lt, "2-1"), true),
            (c(Cmp::Ge, "2"), c(Cmp::Lt, "2-1"), true),
            (c(Cmp::Le, "2"), c(Cmp::Gt, "2-1"), true),
            (c(Cmp::Eq, "1:2"), c(Cmp::Gt, "1:2-1"), true),
            // An empty release is no release.
            (c(Cmp::Eq, "2-"), c(Cmp::Gt, "2-1"), true),
            // Without EQUAL on the release-less side, the ordinary bits
            // decide; with the versions different the rule does not apply.
            (c(Cmp::Gt, "2"), c(Cmp::Eq, "2-1"), false),
            (c(Cmp::Lt, "2"), c(Cmp::Eq, "2-1"), false),
            (c(Cmp::Gt, "2"), c(Cmp::Lt, "2-1"), false),
            (c(Cmp::Eq, "2"), c(Cmp::Gt, "3-1"), false),
            (c(Cmp::Eq, "2"), c(Cmp::Gt, "2"), false),
        ];
        // rpm's overlap is symmetric, so check both operand orders.
        for (provided, wanted, expected) in cases {
            assert_eq!(
                provided.overlaps(&wanted),
                expected,
                "{provided:?} against {wanted:?}"
            );
            assert_eq!(
                wanted.overlaps(&provided),
                expected,
                "{wanted:?} against {provided:?}"
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
    fn supplements_need_every_operand_of_an_and() {
        // `glibc` is in the base image (not added); everything else is.
        let index = TestIndex(&[
            ("glibc", "glibc"),
            ("shared-lib", "shared-lib"),
            ("desktop", "desktop"),
            ("langpacks-en", "langpacks-en"),
            ("langpacks-core-en", "langpacks-core-en"),
        ]);
        let is_added = |id: &str| id != "glibc";
        let triggers = |input: &str| {
            DepExpr::parse(input)
                .expect("parses")
                .supplement_triggers(&index, &is_added)
                .expect("within the clause limit")
        };
        let clauses = |cs: &[&[&[&'static str]]]| {
            SupplementTriggers::Clauses(
                cs.iter()
                    .map(|c| c.iter().map(|g| set(g)).collect())
                    .collect(),
            )
        };
        assert_eq!(
            triggers("(shared-lib and desktop)"),
            clauses(&[&[&["shared-lib"], &["desktop"]]])
        );
        assert_eq!(
            triggers("(glibc and (langpacks-en or langpacks-core-en))"),
            clauses(&[&[&["langpacks-en"]], &[&["langpacks-core-en"]]])
        );
        assert_eq!(
            triggers("(desktop if shared-lib)"),
            clauses(&[&[&["desktop"], &["shared-lib"]]])
        );
        assert_eq!(triggers("(desktop if glibc)"), clauses(&[&[&["desktop"]]]));
        assert_eq!(
            triggers("(glibc or desktop)"),
            SupplementTriggers::AlreadyHolds
        );
        assert_eq!(triggers("(missing and desktop)"), clauses(&[]));
        // An `and` of `or`s past the clause limit is dropped.
        let wide = format!("({})", ["(a or b)"; 7].join(" and "));
        assert_eq!(
            DepExpr::parse(&wide)
                .expect("parses")
                .supplement_triggers(&TestIndex(&[("a", "a"), ("b", "b")]), &|_| true),
            None
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
