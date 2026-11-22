# Redaction belongs on the snapshot object, not at the writer

`inspection-snapshot.json` is `serde_json::to_string_pretty(snap)`. Any
field that holds raw host content and is not scrubbed **on the snapshot
object** ships in the clear, no matter how carefully the sidecar copy of
the same content was scrubbed on its way out.

This is the redaction form of the two-export-paths problem
([export-paths-stage-different-trees](export-paths-stage-different-trees.md)),
and it bites harder, because the symptom is silence rather than a failed
build.

**Read the last section first.** Four fields had this gap, and the
principle written down here did not prevent the second, third and fourth
of them. What prevents them is the pair of tests at the end.

## The instance

`redact()` (`crates/pipeline/src/redaction/engine.rs`) walked
`non_rpm_software.env_files` and `items[].git_remote` and never
`items[].manifest_files`. The only thing that scrubbed a manifest was
`scrub_manifest_auth`, called by `write_language_package_manifests` as it
wrote the sidecar. So in one export, measured on a real host:

```
language-packages/npm/<hash>/package.json   "registry": "https://REDACTED@npm.corp.example.com/"
inspection-snapshot.json                    "registry": "https://deploy:<credential>@npm.corp.example.com/"
```

The refine export path had noticed this and patched it locally, with a
projection over `manifest_files` just before its own
`to_string_pretty`. The scan path never got the equivalent. Two paths,
one of them fixed, is the standing failure mode here.

## Where to put the scrub

**In `redact()`.** It is the one place that owns the snapshot object
before either path serializes it, so a scrub there cannot be inherited
by one path and not the other. A scrub at a writer is, by construction,
a scrub one path can skip.

Corollary: a sidecar-time scrub does not even cover its own path
completely. `write_language_package_manifests` routes by method string
and recognises `python venv`, `pip dist-info`, `npm lockfile` and
`gem lockfile`; a `package.json`-only project (`npm manifest`) hits the
`else { continue }` and is never staged. For those projects the snapshot
JSON is the *only* copy of the manifest in the export, so a writer-side
scrub reaches none of them.

**A warning is never scrubbed.** `redact()` scrubs file contents and the
fields it knows; `snapshot.warnings` serializes as written, and the README
and refine's Services section print each `message`. So a warning names
where a host value is (the unit, the key, the file and line), never the
value: the units collector's unknown-needs warning once copied a whole
`ExecStart=` line, token and all, into `message` and `extra.value`. A
test for a new warning puts a sentinel credential in the host value and
asserts it is absent from every file of both exports and from refine's
view and sections.

## A credential in a URL authority has no generic pattern

`PATTERNS` matches the PostgreSQL, MongoDB, Redis and JDBC URL forms
only, so `https://user:pass@host/` passes the generic pass untouched.
Every place that holds URLs a host can embed a credential in needs a
dedicated scrub in `redact()`: manifests use `scrub_manifest_auth`, proxy
lines `mask_proxy_credentials`, and `.repo` files and their Configuration
rows `scrub_repo_urls`, which applies core's `scrub_url_userinfo`: a URL
begins at each `scheme://`, and its userinfo runs to the last `@` before
the first `/`, `?` or `#`, or the line's end. Never cut a URL at a comma
or other sub-delimiter first: RFC 3986 allows them in userinfo, and
splitting `mirrorlist=` values on commas, as dnf splits list options,
kept `orchid,92`'s tail. A repo URL's credential becomes `REDACTED` in
the authority, so the operator can see where to supply it. A raw
(`--no-redaction`) scan keeps it, by design.

**Diagnostics are not redacted, so name them clean.** A warning, notice
or degraded reason is written at collection, before `redact()` runs, and
`redact()` never reads it. Any repo or key reference formatted into one
goes through `scrub_reference_userinfo` first; the RPM collector's
`DiagnosticNames` also names `REDACTED` a reference dnf's list split cut
out of a comma-holding userinfo, which has no `@` to recognize.

**A `.repo` file's `[section]` headers are never searched.** A repo id
can spell a key (`[copr:...:token:vendor]`), and rewriting it leaves a
file that defines no repo. The engine skips matches on a header line of
a `.repo` file dnf reads (`is_repo_file_path`), and only there: a YAML
flow sequence `[token: x]` elsewhere is a real secret.

## Search forward from the key, never from the start of the line

This was the single defect behind eight separate credential leaks
across six functions, and it always looked the same: the scrubber found
the right **line** and then took the wrong **span**.

```
"publishConfig": { "registry": "https://deploy:s3cret@host/" }
                ^ line.find(':') stops here, so "registry" is read as the value
```

The caller already knows which key routed the line to the scrubber.
Give that to [`value_span_after_key`](../../crates/core/src/redaction.rs)
and let it search forward from the key's end. It has four named
syntaxes — `JsonMember`, `QuotedArgument`, `OptionArgument`,
`Assignment` — for the four forms the callers actually parse, and it is
the only place in the codebase that decides where a value starts.

Three rules it enforces that each cost a leak to learn:

- **Only whitespace may sit between the key and its separator.**
  Anything else means the separator belongs to a different key.
- **A quoted value must open at the first non-whitespace character
  after the separator, and close on the same quote character.** The old
  Gemfile loop tried `"` before `'` and broke on whichever appeared
  first anywhere on the line, so a quoted word in a trailing comment
  won over the source URL.
- **The character right after the key must be the separator**, so a
  prefix match against a longer option cannot take that option's
  argument.

Reconstruct through the located span, never through `replace`: the
Gemfile scrubber used `trimmed.replace(url, &scrubbed)` and rewrote
every occurrence of the matched text on the line, not the one it had
found.

For "every URL on this line" — `Gemfile.lock` — tokenize and replace in
**descending offset order**, so an earlier rewrite cannot invalidate a
later span. Same discipline the regex engine uses.

## Detect on the marker, not on string inequality

A scrubber rebuilds the lines it handles, so the output string can
differ from the input without anything having been removed. Use the
redaction marker's count instead:

```rust
if scrubbed.matches(URL_AUTH_REDACTED_MARKER).count()
    > content.matches(URL_AUTH_REDACTED_MARKER).count()
```

The count comparison rather than a `contains` keeps a second `redact()`
pass over an already-scrubbed snapshot from inventing a finding, and
keeps a credential-free manifest byte-identical through the pass.

**Know what the marker proves.** It answers "did `scrub_url_auth`
fire?" It does **not** answer "is this content now credential-free."
The marker says a scrub ran; the unlocatable report below is what
covers the rest.

## A scrubber that cannot find its value has to say so

This is the half that makes the span fixes durable rather than five
more patches.

The scrubbers are rewriters, not detectors. When one runs, or fails to
run, it emits nothing into `snapshot.redactions`, and three things
follow at once: `secrets-review.md` says nothing, `README.md` prints a
`Secrets redacted` count that does not include it (or omits the row
entirely, since `readme.rs` only prints it when the count is non-zero),
and the state stays eligible for `FullyRedacted` because nothing
unresolved weighs against it. `FullyRedacted` is the one state
`crates/refine/src/tarball.rs` trusts to skip re-redaction on import, so
a snapshot mislabelled that way stays unscrubbed through a round trip.

So: anything added to `redact()` that rewrites content must also push a
`RedactionFinding`. A high-confidence finding is a *resolved* one, which
keeps `FullyRedacted` reachable.

And the inverse, which is the part that was missing. A per-line
scrubber returns one of three answers, not two:

| answer | meaning | reported |
|---|---|---|
| `NotMine` | no key matched; nothing was routed here | no |
| `Rewritten` | the value was located; it may or may not have held a credential | only if the marker count rose |
| `Unlocatable` | **a key matched and the value could not be found** | yes, as an unresolved finding |

The third is the one that used to be folded into the first. A line a
scrubber was asked to handle and could not is reported at
`Confidence::Low`, which the state machine counts as unresolved, which
downgrades the snapshot out of `FullyRedacted`. That is deliberate: the
label the import path trusts must stop being applied over content
nothing examined.

Two constraints on that report:

- **It carries no line text.** The findings are serialized into the
  export and the premise is that the line may hold a credential nothing
  stripped. File, line number and the key that routed it, nothing more.
- **It must not fire on well-formed input.** A located value that
  simply had no credential is silent, and a line no key matched is
  silent. Otherwise every ordinary manifest downgrades the state and
  the signal means nothing. `scrub_compose_secrets` has no unlocatable
  state at all now, because its key locator verifies the separator
  itself — say that rather than inventing a signal to be symmetrical.

## Comment exemptions follow the file's parser

Redaction skips a line that starts with a comment marker, so each marker
is a promise about the parser that reads the file. `#` holds everywhere.
`;` and `//` hold only where that file's parser says so
(`comment_markers` in `redaction/engine.rs`); elsewhere a line such as
`//srv/share/password=x` is data and must be scanned. Decide from the
parser's documentation, not the extension: `.conf` covers both
`smb.conf` (`;` is a comment) and `sshd_config`-style files (it is not).
systemd is the case that needs the path, not the name: `systemd.syntax(7)`
gives units, drop-ins and the daemons' own `.conf` files `#` and `;`
comments, so the test is a `systemd` directory component plus a unit
suffix, a `*.d/*.conf` drop-in, or a `.conf` directly in the `systemd`
directory. Getting it wrong in the narrow direction costs false findings;
in the wide direction it leaks, so widen only with a table row in
`test_non_hash_markers_exempt_only_where_they_are_comment_syntax`.

A match is on a comment line when the **secret's** line starts with a
marker before the match does (`is_comment_line`). Never read the line
the whole match starts on: the AWS pattern consumes the byte before the
key as its boundary, and when that byte was the newline ending a
`# note` line, the comment above exempted the key below it, which shipped
with no finding. Context the match consumed on the secret's own line is
not part of the prefix either, so `#AKIA...` stays redacted: the `#`
there is the pattern's boundary, and it was redacted before keys kept
their context (`test_aws_key_after_a_comment_line_is_redacted`). The JDBC
pattern consumes no boundary, so `#jdbc:...` is a comment line and exempt,
as `#password=x` is. A finding's line is
the secret's line too, counted as newlines before the secret plus one
(`line_number`); `str::lines` on the text before a match counts the
partial line and put every match not at column 0 one line low
(`test_finding_line_is_the_secret_line`).

A boundary after a secret is checked, not matched. The `regex` crate has
no lookahead, and a trailing `(?:[^A-Z0-9]|$)` consumed the separator the
next AWS key's match needed, so of two keys one character apart only the
first was found. The AWS pattern matches up to the key and its `accept`
check rejects a key followed by another key byte
(`test_adjacent_aws_keys_are_each_redacted`).

## A pattern replaces the secret, not its key

Every pattern in `redaction/patterns.rs` that matches anything besides
the secret (a key and its delimiter, a URI's scheme and user, the
boundary byte before an AWS key id) names the secret with a
`(?P<secret>...)` group (`SECRET_GROUP`). Only that group is replaced, so
`proxy_password = hunter2` becomes `proxy_password = REDACTED_PASSWORD_1`
with its key, delimiter and spacing as written. A pattern without the
group is replaced whole, which is right only when the whole match is the
secret: a PEM block, a password hash.
`test_every_pattern_with_context_declares_its_secret_group` fails when a
new pattern forgets the group.

This is one rule in the engine, not a per-pattern template:
`collect_eligible_matches` sets `start..end` and `text` of every
`EligibleMatch` to the secret group, so every call site that replaces
`em.start..em.end` keeps the key, including the ones that call
`token_for` directly. A per-pattern `replacement_template` was the old
mechanism, and three call sites never honoured it.

Overlaps between patterns are decided on the **secret** spans, because
those are what get replaced. Deciding them on the whole match instead
drops any second secret that sits inside one pattern's context but
outside its secret, and that secret then ships: in
`postgres://AKIAABCDEFGHIJKLMNOP:pw@db` the PostgreSQL match runs from
`postgres:` to the `@`, and on whole matches it swallows the AWS key id
used as the user name
(`test_a_match_inside_another_patterns_claim_is_still_redacted`).

When two secrets do overlap, the whole match (`claim`) picks the winner,
larger first, and the winner **absorbs** the loser: its span grows to the
union of both secrets and takes the winner's token. Never drop the loser.
A conflict-free set of replaced spans does not mean every secret byte was
replaced: with the winner's secret alone, `jdbc:...?token=a&password=b`
shipped `a`, because the generic match `a&password=b` overlapped the JDBC
secret `b` and was thrown away. The cost of absorbing is over-redaction
where a pattern's value class overreaches (the generic `\S+` value can
run past the end of a URI password into its `@host`); the cost of
dropping is a credential in the export.

A match can overlap two kept secrets that do not overlap each other, and
so bridge them: in `db_password=postgres://u:a token=b@host` the URI
password `a token=b` touches both generic values. Everything connected
becomes one span, from the smallest start to the largest end, and its
text is read from the content, never stitched from the pieces. Stitching
assumes each pair overlaps; merging the two kept spans before the bridge
broke that and panicked on every redaction path
(`test_a_bridge_between_kept_secrets_merges_all_of_them`,
`prop_redaction_covers_every_captured_secret_without_panicking`).

Absorbing is the backstop, not the design. A pattern whose context can
hold other credentials makes its secret cover them. The JDBC pattern
fires on a URL that carries a credential anywhere, an `@` or a non-empty
`password=` after `scheme://`, and its secret is everything after
`scheme://` through the last such marker on the line and on to the next
whitespace: authority, host, database and query string together. So a
second `password=`, a `token=`, a nested URI and an authority password
are all inside it. That reach is what covers an authority password with
a space in it, and its cost is that a later `password=` or `@` on the
same line takes the assignments in between: stopping before ` key=`
would ship the end of a password such as `p@ss w=x`, which reads the same
as `p@db w=x@host`. It also takes a closing quote or tag that touches the
URL; see "Redaction may take syntax, never leave a secret byte" below.
Splitting the URL into an authority pattern and a query pattern was tried and shipped what the split got wrong: a password
with no user before it (`jdbc:mysql://:pw@db`) and the part of one before
a `?` (`jdbc:mysql://u:pa?ss@db`). The cost is the host and database
name, which a recipient no longer sees
(`test_a_jdbc_url_with_a_credential_is_replaced_after_its_scheme`).
`test_inputs_redacted_by_whole_match_replacement_stay_redacted` holds
every input the old whole-match replacement covered to the same
coverage.

Every pattern goes through that one function, so every filter applies to
every pattern, and the filters read the secret's text, never the key:
comment lines, the NSS/PAM values (`password: files`), and password
hashes in a shadow file. Do not add a second matching path.

**No filter skips a value because it looks like a token.** Keeping the
key means a second pass over redacted text sees
`password=REDACTED_PASSWORD_1`, and skipping anything spelled
`REDACTED_<KIND>_<N>` looked like the way to keep that pass quiet. The
spelling is not provenance: `password=REDACTED_COMPANY_2026` is a
password a host can have, and that filter shipped it with no finding
under `FullyRedacted`. Nothing runs a second pass in production either:
`redact()` runs once, in `scan` and in `run_pipeline`, and `refine` and
`aggregate` load the redacted snapshot without calling it. So a second
pass reports most tokens again and replaces each with a token, usually
itself, but it is **not** guaranteed to return the same text. A token
sits in a context the host's value did not have, and a pattern can match
it differently there: `password=postgres://u:p w@db` becomes
`password=REDACTED_PASSWORD_1@db`, and a second pass lets the generic
`\S+` value run on through `@db` and gives `password=REDACTED_PASSWORD_1`.
A pass can also find fewer secrets than the one before: a JDBC URL
reduced to its scheme has nothing left to match. What a repeat pass
cannot do is bring a secret back, since it only replaces
(`test_a_second_pass_is_not_guaranteed_to_return_the_same_text`). To
make a second pass quiet, carry provenance (the snapshot's redaction
state, or the tokens a run issued) rather than reading it off the text.

Two consumers depended on the key going with the value, and a third
change of this kind must check them:

- **Hint resolution** in `redact()` used to call a high-confidence hint
  resolved when its keyword (`password`, `DB_PASSWORD`) was gone from
  the redacted text. The key now stays, so it asks
  `keyword_survives_with_its_value` instead: every occurrence's whole
  value, past spacing and one `=` or `:`, must be a token this run's
  `CounterRegistry` issued. Both halves matter. A token-spelled prefix is
  not enough: `DB_KEY=REDACTED_PASSWORD_1/hunter2` in a collected `.env`
  matches no pattern and was resolved over `hunter2`. Nor is the
  spelling: only the registry knows what this run replaced
  (`test_hint_with_a_token_prefixed_value_stays_unresolved`).
  Where the value ends depends on the file (`HintValueEnd`). On the
  kernel command line whitespace separates parameters, so the token must
  run to the next whitespace. Anywhere else the value runs to the end of
  the line, so only whitespace or a `#` comment may follow the token:
  `DB_KEY=REDACTED_PASSWORD_1 still-secret` is a `.env` value of two
  words, the second one never replaced. Add a path to the whitespace
  side only when its parser is known to end a value there.
- **Tokens are keyed by the secret's text**, so the same value under two
  keys gets one token, as `CounterRegistry` intends.

## A `key = value` pattern stays on its line

Whitespace around a delimiter is `[ \t]*`, never `\s*`. `\s` matches a
newline, so after an empty value the match runs on into the next line:
the stock RHEL `rhsm.conf` has `proxy_password =` followed by a blank
line and a comment, and the generic pattern took the key, the blank line
and the comment's `#`, leaving a line with no `=` that `configparser`
rejects. With `[ \t]*` the value class (`\S+`) must start on the key's
own line, so an empty value does not match at all. A test that only
checks the secret is gone passes over this. The byte comparison in
`test_redact_leaves_stock_rhsm_conf_parseable` is what proves it; its
`assignment_keys` helper only catches a line with no key before `=` and
is not a `configparser` model.

The same holds for every negated class: `[^:]`, `[^@]` and `[^?]` all
match a newline. The URI patterns use `[^:\r\n]`, `[^@\r\n]` and
`[^?\r\n]`, or a password-less `postgres://db/app` pairs with a
`contact: ops@host` line further down
(`test_uri_patterns_never_cross_a_line_break`). Exclude CR and LF, not
`\s`: a URI password with a space or tab in it is malformed but is still
a password, and `[^@\s]` stopped redacting `postgres://u:two words@db`. Do not exclude `/` either: a password written with a literal `/` reads
the same as a password-less URI with an `@` in its query
(`postgres://host:5432/db?u=a@b`), so excluding it ships the first to
fix the second
(`test_uri_password_with_a_slash_is_covered_at_the_cost_of_a_query_at`).
Only the PEM patterns span lines, on purpose, with `(?s)`.

## Redaction may take syntax, never leave a secret byte

Redaction may over-redact the syntax around a secret. It may never
under-redact the secret. Until redaction parses each file by its format,
no pattern decides where a value ends, or whether it holds a credential,
by inferring the file's syntax from punctuation.

So the generic credential value is `\S+`, and the JDBC secret runs to the
next whitespace after the line's last marker. A quote, `<` or `>` that
touches the value goes with it: Tomcat's `<Resource password="p"/>`
becomes `<Resource password=REDACTED_PASSWORD_1`, and
`url="jdbc:mysql://u:pw@db/app" pool=4` loses its closing quote. Those
outputs are pinned in `test_a_quoted_or_xml_value_loses_its_closing_syntax`,
so the cost is visible and deliberate, and fixing it means format-aware
redaction, not a smarter regex.

Inferring the end was tried, over four review rounds, and every version
shipped bytes `\S+` covered. An unquoted value that stopped at a quote,
`<` or `>` followed by `/`, `,`, `;`, `)`, `]` or `}` kept `/tail` of
`password=ab"/tail`, since those characters are as good in a password as
in markup. Quoted values read inside their quotes kept `hunter2` of
`DB_PASSWORD=""hunter2`, and a value read without its quotes became
`files`, which the NSS/PAM filter then exempted. JDBC patterns keyed on
the character before `jdbc:` needed an escape rule (a backslash escapes
in JSON and is a literal in XML), and setting backslashes aside read
`password=\\` as empty and left `sig=signsecret` in the query. A tag
walker that inferred an attribute's quote stopped inside `pa'ss` and
shipped the authority password. Every one of these inputs is a row in
`test_no_secret_byte_survives_a_reviewed_input`, which asserts the exact
output and that no secret survives anywhere in the serialized snapshot.
A new pattern adds a row there for each input it is meant to fix.

A new pattern beside the others cannot shorten what they cover, because
overlapping secrets absorb one another and none is dropped: a second
pattern can only make a span longer. JSON's `"password": "x"` is such a
pattern, last in `PATTERNS`, since the generic one needs its `=` or `:`
right after the key. Its secret is the whole string, quotes included,
through the next whitespace after the closing quote, so it never takes
less than `\S+` would from the opening quote. A value that loses its
quotes is not reread as `files` and exempted, and the JSON stops parsing,
which costs syntax, not a secret. The cost to findings: a one-line PEM
key in a JSON string now takes the longer match and a password token
rather than a private-key one. The second such pattern, last after it,
takes a quoted value under an unquoted key (`password: "two words"`)
whole, where `\S+` stopped at the space. Like the JSON one, it can only
lengthen a span that `\S+` starts at the same quote.

A secret that runs past its own value can run into the next key, so the
search for a pattern's next match resumes where the last match's
**secret** starts, not where the whole match ends
(`collect_eligible_matches`, for every pattern). With `captures_iter`,
the JSON secret `"alpha","token":"beta` in
`{"password":"alpha","token":"beta gamma"}` moved the search past
`token`, which was never matched, and `gamma` shipped. Resuming at the
secret finds every key from the end of the key before it. A match found
inside an earlier secret overlaps it and is absorbed, so this only
lengthens spans; a pattern whose secret is the whole match has no key
and resumes after the match as before. The property test cannot see
this class: its oracle is the set of matches the engine discovers, so
an undiscovered key is absent from both sides. Each case is a row in
`test_no_secret_byte_survives_a_reviewed_input`.

Resuming inside a secret re-reads it, and keys packed into one value
make that quadratic: in `password=password=...x` every key's secret is
the rest of the line, and the first version found 29,000 matches in a
261 KB line and copied each one's text, about 3.5 GiB. Three rules keep
the search linear without replacing less. **Offsets, not copies:** an
`EligibleMatch` is ranges and a line number through collection,
filtering and the merge, filters read borrowed slices, and a caller
copies only a surviving span's text (`EligibleMatch::secret`).
**Skip covered suffixes:** a secret inside the range the pattern's
overlapping eligible secrets already cover is not kept, since it adds
no byte and loses the merge to the secret it sits in, and once that
range reaches the line break, the search moves to the break, since no
keyed pattern's match crosses one. **Fail toward over-redaction:** a
value that stops before its line does (`password=password=...x tail`)
still has its inner keys each read to `x`, so after
`MAX_REDUNDANT_RESCANS_PER_LINE` matches that start inside text already
read and add nothing, the rest of the line becomes one unfiltered
secret. Any bound added here must work that way: past it, replace more,
never stop searching. `test_bounded_search_covers_what_an_unbounded_one_finds`
and its property test check the bounded search against an unbounded
one, and `test_dense_repeated_keys_redact_in_bounded_time` times the
dense inputs. Line facts come from a forward `LineCursor` for the same
reason: counting newlines from the start of the content for every match
was quadratic too.

`SecretPattern::accept` exists for a condition the regex cannot state
because the `regex` crate has no lookahead: the AWS pattern's check that
no key byte follows the key. Use it only for a fact about bytes that
cannot be part of the secret. A check that rejects a match on inferred
syntax drops the whole match, and nothing falls back to cover it.

## Content parsed after redaction is unknown when the pass changed it

Some content is parsed *after* `redact()` has run: the sysctl
explanation reads `kernel_boot.sysctl` files and runtime values to say
what each key resolves to and whether it drifts. Parsing scrubbed text
gives a confident wrong answer. A key kept beside a token parses as a
setting whose value the host never had, a whole-match token (a PEM
block) leaves text that parses as a different setting or as none, and a
token compared with a host value can read as equal or as drift.

The rule: **when the pass changed a piece of content, what it says is
unknown, and nothing is parsed from the scrubbed text.** The pass records
the change on the object, and the consumer reads the record rather than
the text:

- `SysctlFileBody::Read.redacted` is set when the content changed.
  While that file survives shadowing (no same-named file in a
  higher-priority directory replaces it), `SysctlSection::effective()`
  returns `Unknown` with an input naming the file. Its lines contribute
  nothing to the as-written list either, because line numbers after a
  multi-line replacement are not the host's.
- A runtime value with any match is removed from `runtime` and its key
  added to `redacted_runtime`. No scrubbed remainder is stored, because
  nothing would read it, and the explanation names the key as unknown.
- A link target is a path, not a line systemd-sysctl parses, so a change
  there scrubs it without setting `redacted`.

The cost is accepted: a host whose sysctl files hold a secret-bearing
key gets no values or drift for any key (Mark, 2026-11-11). The
alternative is an explanation built from text the host never had.

The pass is the config pass reused, not a new scrubber
(`redact_sysctl_section` in `redaction/engine.rs`): the same patterns,
eligibility and comment exemptions under the field's path (runtime
values under `/proc/sys`), the same secret-span replacement and overlap
arbitration, the snapshot's one counter registry, and findings into `all_findings` like
any other, one per match per path. An `/etc` sysctl file that is also a
config entry is scrubbed in both copies, but the config pass has already
recorded its matches, so the sysctl pass drops a content finding equal
to one already recorded at that path. It does so only when the two
copies held byte-identical text before scrubbing: a finding carries no
matched text, so two different secrets on the same line read as equal
findings, and a file that changed between the two reads must keep both. Recording both listed the file
twice in `secrets-review.md` and counted each match twice wherever
`redactions` is counted. A new consumer that parses content after redaction follows
the same rule: give the pass a flag to set, and read the flag.

## Testing it

Assert over the **whole export directory**, not over one named file:
walk every file under the output dir and assert none contains the
fixture credential
(`render::tests::no_export_artifact_carries_a_manifest_credential`). The
leak above lived for five releases under a suite that checked the
sidecar. A test naming one file cannot see the copy two directories up.

Pair it with a credential-free fixture asserting the content comes back
**byte-identical**, which is what catches the reformat-as-scrub
confusion above.

## The class, and the two tests that close it

Four fields had this gap, found one at a time:

| field | also reached | found by |
|---|---|---|
| `non_rpm_software.items[].manifest_files` | the tarball, `aggregate` | audit |
| `containers.compose_files[].raw_content` | — | fixing the first |
| `containers.quadlet_units[].content` | **the built image** | review of the first fix |
| `scheduled_tasks.*.timer_content` | `config/etc/systemd/system/` | review of the first fix |
| `scheduled_tasks.systemd_timers[].description` | the audit report | the sweep test below |

`SystemdTimer` has carried no file content since phase 3 of the units
plan: a local timer's file is a config entry or a drop-in row, scrubbed
by those passes. `timer_content` and `service_content` remain on
`generated_timer_units` only.

The sysctl section (`kernel_boot.sysctl`) was classified when it was
added, not found later: `SysctlSection.runtime` values,
`SysctlFileBody::Read.content` and `link_target`, and
`SysctlFileBody::OutsideLink.target` are scrubbed;
`SysctlFile.dir` and `name`, `Read.redacted` and `redacted_runtime` are
exempt. No sysctl type has a `Default`, so its patterns sit in
never-called functions the compiler still checks for completeness.

None of these was a parsing bug. Each was a content-bearing field nobody
wired into `redact()`. The quadlet one is the sharpest: a
`COPY quadlet/ /etc/containers/systemd/` put an unscrubbed
`Environment=DB_PASSWORD=` line into the built image, at a path systemd
reads at boot, with the snapshot stamped `FullyRedacted` and no finding
raised — and the identical content *is* scrubbed when it arrives as
`services.drop_ins[].content`.

Two tests in `crates/pipeline/src/render/mod.rs` now hold it shut, and
they need each other:

1. **`every_field_of_a_content_bearing_type_is_classified`** destructures
   every content-bearing snapshot struct with no `..` in the pattern, so
   **adding a field stops the build** until someone writes it into the
   scrubbed or exempt group with a reason. A compile error, not a
   convention.
2. **`exactly_the_exempt_fields_survive_the_export`** plants a distinct
   sentinel in every one of those fields, runs `redact()` and
   `render_all()`, and asserts that the set surviving anywhere under the
   export directory is **exactly** the declared exempt set.

Assert only that scrubbed fields vanish and you get an untested half.
That is precisely how `systemd_timers[].description` got through: it was
classified exempt on the reasoning that a `Description=` is prose rather
than a value, nothing planted a secret in it, and a real scan then put
one in the snapshot and in the rendered audit report. **An exemption
nothing exercises is an assumption, not a decision** — so the exempt
fields are planted too, and the test fails if one of them stops
surviving, which means someone wired it and the classification is stale.

When adding a field to a snapshot type: the compile error tells you to
classify it; if you classify it scrubbed, add a sentinel and wire the
field in `redact()`; if you classify it exempt, add a sentinel anyway
and say why in the comment beside it.
