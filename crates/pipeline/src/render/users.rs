//! User and group renderers — produces kickstart, blueprint TOML, and
//! Containerfile fragments from the enriched users/groups data.

use inspectah_core::snapshot::InspectionSnapshot;
use std::collections::{HashMap, HashSet};

use super::comment::LINE_TERMINATORS;
use super::language_packages::sanitize_for_comment;
use super::safety::{AccountName, CryptHash, ExportPath, double_quoted_word_refusal};
use super::var_dirs::EffectiveBacking;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a string field from a serde_json::Value.
fn str_field<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

/// Extract a u32 numeric field from a serde_json::Value (JSON numbers are f64).
fn uid_field(v: &serde_json::Value, key: &str) -> u32 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(0.0) as u32
}

/// Collect the `supplementary_groups` array as Vec<String>.
fn supp_groups(v: &serde_json::Value) -> Vec<String> {
    v.get("supplementary_groups")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Collect the `ssh_keys` array as Vec<String>.
fn ssh_keys(v: &serde_json::Value) -> Vec<String> {
    v.get("ssh_keys")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Return only custom groups (source == "custom") that have `include` true.
fn custom_groups(snap: &InspectionSnapshot) -> Vec<&serde_json::Value> {
    let ug = match &snap.users_groups {
        Some(u) => u,
        None => return Vec::new(),
    };
    ug.groups
        .iter()
        .filter(|g| {
            let src = str_field(g, "source");
            let include = g.get("include").and_then(|v| v.as_bool()).unwrap_or(true);
            src == "custom" && include
        })
        .collect()
}

/// Return users that are included (include != false).
fn included_users(snap: &InspectionSnapshot) -> Vec<&serde_json::Value> {
    let ug = match &snap.users_groups {
        Some(u) => u,
        None => return Vec::new(),
    };
    ug.users
        .iter()
        .filter(|u| u.get("include").and_then(|v| v.as_bool()).unwrap_or(true))
        .collect()
}

/// Why `useradd` cannot be emitted for `u`, or `None` when every field
/// on its line is proved: the name and each supplementary group are
/// [`AccountName`]s, and a home or shell, when present, is an
/// [`ExportPath`]. Dropping one field instead would create a user unlike
/// the host's, so a refusal costs the whole user.
fn useradd_refusal(u: &serde_json::Value) -> Option<&'static str> {
    if AccountName::parse(str_field(u, "name")).is_err() {
        return Some("the name is not a valid account name");
    }
    if supp_groups(u)
        .iter()
        .any(|g| AccountName::parse(g).is_err())
    {
        return Some("a supplementary group is not a valid group name");
    }
    let home = str_field(u, "home");
    if !home.is_empty() && ExportPath::parse(home).is_err() {
        return Some("the home directory is not a plain absolute path");
    }
    let shell = str_field(u, "shell");
    if !shell.is_empty() && ExportPath::parse(shell).is_err() {
        return Some("the login shell is not a plain absolute path");
    }
    None
}

/// `s` as a TOML basic string, quotes included.
///
/// Every value in the blueprint fragment goes through this, so none can
/// close its string and start a key. Escaping keeps the TOML whole; it
/// says nothing about what the blueprint's consumer does with the value.
/// So the account names, home, shell and hash are proved first, as the
/// kickstart and Containerfile prove them, and only the SSH key relies on
/// this escaping alone: a key holds spaces and is carried as it is.
///
/// `"` and `\` get a backslash, and every control character and line
/// terminator is written as a `\uXXXX` escape. U+0085, U+2028 and
/// U+2029 are legal raw in a TOML string, but an editor may break the
/// line there, so they are escaped too.
pub(super) fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() || LINE_TERMINATORS.contains(&c) => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// 1. Kickstart renderer
// ---------------------------------------------------------------------------

/// Render a kickstart fragment for user/group provisioning.
///
/// Emits custom groups first, then user directives with `--uid`, `--gid`,
/// `--groups`, and conditional `--iscrypted --password=` and `sshkey` lines.
pub fn render_kickstart(snap: &InspectionSnapshot) -> String {
    let mut lines: Vec<String> = Vec::new();

    let groups = custom_groups(snap);
    let users = included_users(snap);

    if groups.is_empty() && users.is_empty() {
        return String::new();
    }

    // Groups first
    for g in &groups {
        let name = str_field(g, "name");
        let gid = uid_field(g, "gid");
        if name.is_empty() {
            continue;
        }
        // Every value on a kickstart directive line is proved, as on the
        // Containerfile's `groupadd` and `useradd` lines: a space adds an
        // option and a line break adds a directive.
        if AccountName::parse(name).is_err() {
            lines.push(format!(
                "# FIXME: group {} not created: the name is not a valid group name",
                sanitize_for_comment(name)
            ));
            continue;
        }
        if gid > 0 {
            lines.push(format!("group --name={name} --gid={gid}"));
        } else {
            lines.push(format!("group --name={name}"));
        }
    }

    // Users
    for u in &users {
        let name = str_field(u, "name");
        if name.is_empty() {
            continue;
        }
        // A refusal costs the whole user, and its keys, as in the
        // Containerfile: dropping one field would create a different user.
        if let Some(reason) = useradd_refusal(u) {
            lines.push(format!(
                "# FIXME: user {} not created: {reason}",
                sanitize_for_comment(name)
            ));
            continue;
        }
        let uid = uid_field(u, "uid");
        let gid = uid_field(u, "gid");
        let groups_list = supp_groups(u);

        let shell = str_field(u, "shell");
        let home = str_field(u, "home");

        let mut opts = format!("user --name={name}");
        if uid > 0 {
            opts.push_str(&format!(" --uid={uid}"));
        }
        if gid > 0 {
            opts.push_str(&format!(" --gid={gid}"));
        }
        if !shell.is_empty() {
            opts.push_str(&format!(" --shell={shell}"));
        }
        if !home.is_empty() {
            opts.push_str(&format!(" --homedir={home}"));
        }
        if !groups_list.is_empty() {
            opts.push_str(&format!(" --groups={}", groups_list.join(",")));
        }

        // Password (conditional). A refused hash costs only the password.
        let pw = str_field(u, "password_hash");
        let mut pw_fixme = None;
        if !pw.is_empty() {
            match CryptHash::parse(pw) {
                Ok(hash) => opts.push_str(&format!(" --iscrypted --password={}", hash.as_str())),
                Err(_) => {
                    pw_fixme = Some(format!(
                        "# FIXME: password for user {name} not set: the hash is not a crypt(3) string"
                    ))
                }
            }
        }

        lines.push(opts);
        lines.extend(pw_fixme);

        // SSH keys: inside double quotes, so only what ends or acts there
        // is refused; a key's spaces are legitimate.
        let keys = ssh_keys(u);
        for key in &keys {
            match double_quoted_word_refusal(key) {
                None => lines.push(format!("sshkey --username={name} \"{key}\"")),
                Some(why) => lines.push(format!(
                    "# FIXME: SSH key for user {name} not written: the key {why}: {}",
                    sanitize_for_comment(key)
                )),
            }
        }
    }

    super::comment::join_lines(&lines)
}

// ---------------------------------------------------------------------------
// 2. Blueprint TOML renderer
// ---------------------------------------------------------------------------

/// Render a blueprint TOML fragment for user/group provisioning.
///
/// Emits `[[customizations.group]]` blocks, then `[[customizations.user]]` blocks.
/// A group, user or password the kickstart refuses gets the same `# FIXME`
/// here and is left out, so the two fragments carry the same accounts.
/// Single `key` field for the first SSH key; emits a comment when > 1 key.
pub fn render_blueprint_toml(snap: &InspectionSnapshot) -> String {
    let mut lines: Vec<String> = Vec::new();

    let groups = custom_groups(snap);
    let users = included_users(snap);

    if groups.is_empty() && users.is_empty() {
        return String::new();
    }

    // Groups
    for g in &groups {
        let name = str_field(g, "name");
        let gid = uid_field(g, "gid");
        if name.is_empty() {
            continue;
        }
        // TOML escaping keeps the name in its string, but the blueprint's
        // consumer still creates the account, so the name gets the proof
        // the kickstart and the Containerfile give it.
        if AccountName::parse(name).is_err() {
            lines.push(format!(
                "# FIXME: group {} not created: the name is not a valid group name",
                sanitize_for_comment(name)
            ));
            continue;
        }
        lines.push("[[customizations.group]]".into());
        lines.push(format!("name = {}", toml_string(name)));
        if gid > 0 {
            lines.push(format!("gid = {gid}"));
        }
        lines.push(String::new());
    }

    // Users
    for u in &users {
        let name = str_field(u, "name");
        if name.is_empty() {
            continue;
        }
        // The same users as the kickstart: a refusal costs the whole user.
        if let Some(reason) = useradd_refusal(u) {
            lines.push(format!(
                "# FIXME: user {} not created: {reason}",
                sanitize_for_comment(name)
            ));
            continue;
        }
        let uid = uid_field(u, "uid");
        let gid = uid_field(u, "gid");
        let groups_list = supp_groups(u);
        let home = str_field(u, "home");
        let shell = str_field(u, "shell");

        lines.push("[[customizations.user]]".into());
        lines.push(format!("name = {}", toml_string(name)));
        if uid > 0 {
            lines.push(format!("uid = {uid}"));
        }
        if gid > 0 {
            lines.push(format!("gid = {gid}"));
        }
        if !home.is_empty() {
            lines.push(format!("home = {}", toml_string(home)));
        }
        if !shell.is_empty() {
            lines.push(format!("shell = {}", toml_string(shell)));
        }
        if !groups_list.is_empty() {
            lines.push(format!(
                "groups = [{}]",
                groups_list
                    .iter()
                    .map(|g| toml_string(g))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        // Password. A refused hash costs only the password, as in the
        // kickstart.
        let pw = str_field(u, "password_hash");
        if !pw.is_empty() {
            match CryptHash::parse(pw) {
                Ok(hash) => lines.push(format!("password = {}", toml_string(hash.as_str()))),
                Err(_) => lines.push(format!(
                    "# FIXME: password for user {name} not set: the hash is not a crypt(3) string"
                )),
            }
        }

        // SSH key — blueprint supports a single `key` field
        let keys = ssh_keys(u);
        if let Some(first_key) = keys.first() {
            lines.push(format!("key = {}", toml_string(first_key)));
            if keys.len() > 1 {
                lines.push(format!(
                    "# NOTE: {} additional SSH key(s) cannot be expressed in blueprint TOML (single key only)",
                    keys.len() - 1
                ));
            }
        }

        lines.push(String::new());
    }

    super::comment::join_lines(&lines)
}

// ---------------------------------------------------------------------------
// 3. Containerfile users renderer
// ---------------------------------------------------------------------------

/// Where the users block writes the tmpfiles.d lines that create homes.
/// `/usr` is image content, replaced on every update; a file in
/// `/etc/tmpfiles.d/` would go through the 3-way merge instead.
const USERS_TMPFILES_CONF: &str = "/usr/lib/tmpfiles.d/inspectah-users.conf";

/// The mode of a created home: the RHEL bootc bases' `HOME_MODE`.
const HOME_MODE: &str = "0700";

/// The lines above the group lines, emitted once when there is one.
const GROUP_COMMENT: [&str; 5] = [
    "# Each group keeps the host's gid, so ownership the export writes by number",
    "# still matches. A base image entry with the same name and gid is kept, and",
    "# copied from /usr/lib/group into /etc/group when useradd -G needs it there.",
    "# A group whose name or gid the image already uses differently stops the",
    "# build: set its users to Skip in refine, or renumber it on the host and re-scan.",
];

/// The first lines of the block: what happens to these accounts on a host
/// that changed `/etc/passwd` or `/etc/group` locally.
const UPDATE_COMMENT: [&str; 4] = [
    "# Users and groups are written to /etc/passwd and /etc/group at build time.",
    "# A host that changed either file locally keeps its own copy on update, so",
    "# a user, group or membership added or changed in a later image does not",
    "# reach that host.",
];

/// The lines above the home file's `printf`.
const HOME_COMMENT: [&str; 3] = [
    "# Home directories are created at boot by systemd-tmpfiles, not in the image.",
    "# tmpfiles resets their owner and mode on every boot; to change that on one",
    "# host, put a file named inspectah-users.conf in /etc/tmpfiles.d/.",
];

/// The first lines of the keys comment.
const KEYS_COMMENT: [&str; 2] = [
    "# SSH keys are not part of this image. Provision them at install time with",
    "# kickstart (sshkey --username=<user>), cloud-init, or a systemd credential.",
];

/// The keys comment's third line, when the scan kept keys.
const KEPT_KEYS_COMMENT: &str =
    "# inspectah-users.ks has the sshkey lines for the keys this scan kept.";

/// The users the Containerfile creates, each proved once. Every line the
/// users block writes, and every account name `inspectah-var.conf` may use
/// for a user, comes from `accepted`.
pub(crate) struct ContainerfileUserPlan {
    accepted: Vec<PlannedUser>,
    /// In user order, each rendered where its `useradd` line would be.
    refused: Vec<RefusedUser>,
    /// Deduplicated, first appearance wins, accepted users only.
    groups: Vec<PlannedGroup>,
}

/// A group an accepted user names, with the host's gid.
struct PlannedGroup {
    name: AccountName,
    gid: u32,
    /// Named in some accepted user's `-G`: the line also copies the
    /// entry into `/etc/group`.
    in_supplementary: bool,
}

/// How an accepted user's home comes to exist on a deployed host.
#[derive(Debug, PartialEq, Eq)]
enum HomeRule {
    /// `d <path> 0700 <user> <primary_group> -` in inspectah-users.conf.
    Tmpfiles(ExportPath),
    /// A `/var` home the export already creates: the `/var` fragment has a
    /// rule for exactly this path.
    VarFragment,
    /// The home is `/`.
    Exists,
    /// The "not created by this image" comment.
    NotCreated,
}

/// A user's password as the `chpasswd` line can carry it.
enum PlannedPassword {
    None,
    Hash(CryptHash),
    /// The snapshot's hash is not a crypt(3) string: a FIXME, no entry.
    NotCrypt,
}

/// The proved fields of one `useradd` line and what follows from it.
struct PlannedUser {
    name: AccountName,
    /// `0` when the snapshot has none, and then no `-u`.
    uid: u32,
    primary_group: AccountName,
    supplementary: Vec<AccountName>,
    /// The primary group and each supplementary group, with its gid.
    named_groups: Vec<(AccountName, u32)>,
    shell: Option<ExportPath>,
    home: Option<ExportPath>,
    home_rule: HomeRule,
    password: PlannedPassword,
    ssh_key_count: u64,
    /// The scan kept this user's keys (`ssh_keys`), so the kickstart
    /// fragment has `sshkey` lines for them.
    keys_kept: bool,
}

/// A user the block cannot create, and why.
struct RefusedUser {
    name: String,
    reason: String,
    /// How many accepted users come before it, which places its FIXME.
    position: usize,
}

impl ContainerfileUserPlan {
    /// Build the plan from the users with `containerfile_strategy ==
    /// "useradd"` and `include != false`, in snapshot order.
    pub(crate) fn from_snapshot(snap: &InspectionSnapshot) -> Self {
        let mut accepted: Vec<PlannedUser> = Vec::new();
        let mut refused: Vec<RefusedUser> = Vec::new();
        // The gid each accepted user's groups have, to refuse a later user
        // that disagrees (possible only in a fleet aggregate).
        let mut accepted_gids: HashMap<String, u32> = HashMap::new();
        let mut var_rules: Option<HashSet<String>> = None;

        for u in included_users(snap) {
            if str_field(u, "containerfile_strategy") != "useradd" {
                continue;
            }
            let name = str_field(u, "name");
            if name.is_empty() {
                continue;
            }
            match plan_user(u, &accepted_gids) {
                Ok(mut user) => {
                    user.home_rule = home_rule(user.home.as_ref(), |path| {
                        var_rules
                            .get_or_insert_with(|| var_fragment_paths(snap))
                            .contains(path)
                    });
                    for (group, gid) in &user.named_groups {
                        accepted_gids
                            .entry(group.as_str().to_string())
                            .or_insert(*gid);
                    }
                    accepted.push(user);
                }
                Err(reason) => refused.push(RefusedUser {
                    name: name.to_string(),
                    reason,
                    position: accepted.len(),
                }),
            }
        }

        let named_in_supplementary: HashSet<&str> = accepted
            .iter()
            .flat_map(|u| u.supplementary.iter().map(AccountName::as_str))
            .collect();
        let mut groups: Vec<PlannedGroup> = Vec::new();
        for user in &accepted {
            for (group, gid) in &user.named_groups {
                if groups.iter().any(|g| g.name == *group) {
                    continue;
                }
                groups.push(PlannedGroup {
                    name: group.clone(),
                    gid: *gid,
                    in_supplementary: named_in_supplementary.contains(group.as_str()),
                });
            }
        }

        Self {
            accepted,
            refused,
            groups,
        }
    }

    /// Whether the Containerfile creates the account `name`, so a
    /// tmpfiles.d line may name it as an owner.
    pub(crate) fn creates_user(&self, name: &str) -> bool {
        self.accepted.iter().any(|u| u.name.as_str() == name)
    }

    /// The gid group `name` is guaranteed to have in the image: a group
    /// line gives the image that name with that gid or stops the build.
    /// `None` for any other group, which the image may lack, so a
    /// tmpfiles.d line must not name it.
    pub(crate) fn guaranteed_gid(&self, name: &str) -> Option<u32> {
        self.groups
            .iter()
            .find(|g| g.name.as_str() == name)
            .map(|g| g.gid)
    }

    /// The section's body, without its header.
    fn body(&self) -> Vec<String> {
        let mut body: Vec<String> = Vec::new();
        if self.accepted.is_empty() && self.refused.is_empty() {
            return body;
        }
        body.extend(UPDATE_COMMENT.iter().map(|l| l.to_string()));

        // Group lines.
        if !self.groups.is_empty() {
            body.extend(GROUP_COMMENT.iter().map(|l| l.to_string()));
            for group in &self.groups {
                body.extend(group_lines(group));
            }
        }

        // `useradd` lines, each refusal in its place.
        for (i, user) in self.accepted.iter().enumerate() {
            body.extend(self.refusals_at(i));
            body.push(useradd_line(user));
        }
        body.extend(self.refusals_at(self.accepted.len()));

        body.extend(self.chpasswd_lines());
        body.extend(self.home_lines());
        body.extend(self.keys_lines());
        body
    }

    /// The FIXME of each refused user that comes after `position`
    /// accepted users.
    fn refusals_at(&self, position: usize) -> impl Iterator<Item = String> + '_ {
        self.refused
            .iter()
            .filter(move |r| r.position == position)
            .map(|r| {
                format!(
                    "# FIXME: user {} not created: {}",
                    sanitize_for_comment(&r.name),
                    // Fixed text, and only proved names and numbers in it.
                    r.reason
                )
            })
    }

    fn chpasswd_lines(&self) -> Vec<String> {
        let mut body = Vec::new();
        let with_password: Vec<&PlannedUser> = self
            .accepted
            .iter()
            .filter(|u| !matches!(u.password, PlannedPassword::None))
            .collect();
        if with_password.is_empty() {
            return body;
        }
        body.push(
            "# WARNING: Embedding password hashes in a Containerfile is a security risk.".into(),
        );
        body.push("# Consider using a secrets manager or deploy-time provisioning instead.".into());
        let mut entries: Vec<String> = Vec::new();
        for user in with_password {
            let name = user.name.as_str();
            match &user.password {
                // Both halves are proved free of quotes, `:` and line
                // ends, so each record is one single-quoted word.
                PlannedPassword::Hash(hash) => entries.push(format!("'{name}:{}'", hash.as_str())),
                PlannedPassword::NotCrypt => body.push(format!(
                    "# FIXME: password for user {name} not set: the hash is not a crypt(3) string"
                )),
                PlannedPassword::None => {}
            }
        }
        // One record per `printf` argument: `echo` does not expand `\n`
        // under bash, which would have joined every record into one.
        if !entries.is_empty() {
            body.push(format!(
                "RUN printf '%s\\n' {} | chpasswd -e",
                entries.join(" ")
            ));
        }
        body
    }

    fn home_lines(&self) -> Vec<String> {
        let mut body = Vec::new();
        let tmpfiles: Vec<String> = self
            .accepted
            .iter()
            .filter_map(|u| match &u.home_rule {
                HomeRule::Tmpfiles(path) => Some(format!(
                    "d {} {HOME_MODE} {} {} -",
                    path.as_str(),
                    u.name.as_str(),
                    u.primary_group.as_str()
                )),
                _ => None,
            })
            .collect();
        if !tmpfiles.is_empty() {
            body.extend(HOME_COMMENT.iter().map(|l| l.to_string()));
            // Every value on a line is proved (`ExportPath`, `AccountName`,
            // a fixed mode), so none holds a `'`.
            if let [line] = tmpfiles.as_slice() {
                body.push(format!(
                    "RUN printf '%s\\n' '{line}' > {USERS_TMPFILES_CONF}"
                ));
            } else {
                body.push("RUN printf '%s\\n' \\".into());
                body.extend(tmpfiles.iter().map(|l| format!("    '{l}' \\")));
                body.push(format!("    > {USERS_TMPFILES_CONF}"));
            }
        }
        for user in &self.accepted {
            if user.home_rule != HomeRule::NotCreated {
                continue;
            }
            body.push(match &user.home {
                Some(home) => format!(
                    "# {}'s home, {}, is not created by this image.",
                    user.name.as_str(),
                    home.as_str()
                ),
                None => format!(
                    "# {} has no home directory recorded; none is created by this image.",
                    user.name.as_str()
                ),
            });
        }
        body
    }

    fn keys_lines(&self) -> Vec<String> {
        let mut body = Vec::new();
        let with_keys: Vec<&PlannedUser> = self
            .accepted
            .iter()
            .filter(|u| u.ssh_key_count > 0)
            .collect();
        if with_keys.is_empty() {
            return body;
        }
        body.extend(KEYS_COMMENT.iter().map(|l| l.to_string()));
        if with_keys.iter().any(|u| u.keys_kept) {
            body.push(KEPT_KEYS_COMMENT.into());
        }
        for user in with_keys {
            let count = match user.ssh_key_count {
                1 => "1 key".to_string(),
                n => format!("{n} keys"),
            };
            let home = user.home.as_ref().map_or("~", ExportPath::as_str);
            body.push(format!(
                "# {} had {count} in {home}/.ssh/authorized_keys on the host.",
                user.name.as_str()
            ));
        }
        body
    }
}

/// Prove one user's `useradd` line, or say why it cannot be written.
/// The user's home rule is left for the caller to decide.
fn plan_user(
    u: &serde_json::Value,
    accepted_gids: &HashMap<String, u32>,
) -> Result<PlannedUser, String> {
    if let Some(reason) = useradd_refusal(u) {
        return Err(reason.to_string());
    }
    // `useradd_refusal` proved the name and every supplementary group.
    let name = AccountName::parse(str_field(u, "name"))
        .map_err(|_| "the name is not a valid account name".to_string())?;
    let supplementary: Vec<AccountName> = supp_groups(u)
        .iter()
        .filter_map(|g| AccountName::parse(g).ok())
        .collect();

    let Some(primary) = u.get("primary_group").and_then(|v| v.as_str()) else {
        let gid = u
            .get("gid")
            .and_then(|v| v.as_u64())
            .map_or_else(|| "unknown".to_string(), |g| g.to_string());
        return Err(format!(
            "its primary group (gid {gid}) has no entry in the host's /etc/group"
        ));
    };
    let primary_group = AccountName::parse(primary).map_err(|_| INVALID_GROUP.to_string())?;
    let recorded = u.get("group_gids").and_then(|v| v.as_object());
    if recorded.is_some_and(|m| m.keys().any(|k| AccountName::parse(k).is_err())) {
        return Err(INVALID_GROUP.to_string());
    }

    let mut named_groups: Vec<(AccountName, u32)> = Vec::new();
    for group in std::iter::once(&primary_group).chain(&supplementary) {
        let gid = recorded
            .and_then(|m| m.get(group.as_str()))
            .and_then(|v| v.as_u64())
            .and_then(|g| u32::try_from(g).ok())
            .ok_or_else(|| "a group it names has no recorded gid".to_string())?;
        if let Some(earlier) = accepted_gids.get(group.as_str())
            && *earlier != gid
        {
            return Err(format!(
                "group {} has gid {gid} here and {earlier} for an earlier user",
                group.as_str()
            ));
        }
        named_groups.push((group.clone(), gid));
    }

    let path = |key| {
        let value = str_field(u, key);
        (!value.is_empty())
            .then(|| ExportPath::parse(value).ok())
            .flatten()
    };
    let password = match str_field(u, "password_hash") {
        "" => PlannedPassword::None,
        hash => match CryptHash::parse(hash) {
            Ok(hash) => PlannedPassword::Hash(hash),
            Err(_) => PlannedPassword::NotCrypt,
        },
    };
    let user = PlannedUser {
        name,
        uid: uid_field(u, "uid"),
        primary_group,
        supplementary,
        named_groups,
        shell: path("shell"),
        home: path("home"),
        home_rule: HomeRule::NotCreated,
        password,
        ssh_key_count: u.get("ssh_key_count").and_then(|v| v.as_u64()).unwrap_or(0),
        keys_kept: !ssh_keys(u).is_empty(),
    };
    Ok(user)
}

/// The refusal for a group name `AccountName` does not accept.
const INVALID_GROUP: &str = "a group name is not valid";

/// Which rule creates `home` on a deployed host. `var_rule` says whether
/// the `/var` fragment has an active rule for exactly a path.
fn home_rule(home: Option<&ExportPath>, var_rule: impl FnOnce(&str) -> bool) -> HomeRule {
    let Some(home) = home else {
        return HomeRule::NotCreated;
    };
    let path = home.as_str().trim_end_matches('/');
    if path.is_empty() {
        return HomeRule::Exists;
    }
    // `/home` is a symlink to `var/home` on the base, so the real path is
    // written, as the passwd entry's home resolves.
    let under_var_home = path
        .strip_prefix("/home/")
        .or_else(|| path.strip_prefix("/var/home/"))
        .filter(|rest| !rest.is_empty());
    if let Some(rest) = under_var_home {
        return match ExportPath::parse(&format!("/var/home/{rest}")) {
            Ok(path) => HomeRule::Tmpfiles(path),
            Err(_) => HomeRule::NotCreated,
        };
    }
    if path.starts_with("/var/") && var_rule(path) {
        return HomeRule::VarFragment;
    }
    HomeRule::NotCreated
}

/// The `/var` directories the export has a creator or an active rule
/// for: an effective backing of `Carried` or `Provision`.
fn var_fragment_paths(snap: &InspectionSnapshot) -> HashSet<String> {
    super::var_dirs::effective_var_backing(snap)
        .into_iter()
        .filter(|o| {
            matches!(
                o.outcome,
                EffectiveBacking::Carried | EffectiveBacking::Provision { .. }
            )
        })
        .map(|o| o.path)
        .collect()
}

/// One group's line: kept when the image has the name with the host's gid,
/// copied into `/etc/group` when a `-G` needs it there, created otherwise,
/// and a stopped build with a message when the image uses the name or the
/// gid differently.
fn group_lines(group: &PlannedGroup) -> [String; 2] {
    let name = group.name.as_str();
    let gid = group.gid;
    // `.` is the only regular-expression character `AccountName` admits.
    let pattern = format!("'^{}:[^:]*:{gid}:'", name.replace('.', "\\."));
    let first = if group.in_supplementary {
        format!(
            "RUN grep -q {pattern} /etc/group || getent group {name} | grep {pattern} >> /etc/group || groupadd -g {gid} {name} || \\"
        )
    } else {
        format!("RUN getent group {name} | grep -q {pattern} || groupadd -g {gid} {name} || \\")
    };
    [
        first,
        format!(
            "    {{ echo 'group {name} (gid {gid}): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }}"
        ),
    ]
}

fn useradd_line(user: &PlannedUser) -> String {
    // `-M`: the bases set `CREATE_HOME yes`, and `/home` there is a
    // symlink to a `/var/home` that does not exist at build time.
    let mut cmd = "RUN useradd -M".to_string();
    if user.uid > 0 {
        cmd.push_str(&format!(" -u {}", user.uid));
    }
    cmd.push_str(&format!(" -g {}", user.primary_group.as_str()));
    if !user.supplementary.is_empty() {
        let groups: Vec<&str> = user.supplementary.iter().map(AccountName::as_str).collect();
        cmd.push_str(&format!(" -G {}", groups.join(",")));
    }
    // Proved by `useradd_refusal`, and quoted as well.
    if let Some(shell) = &user.shell {
        cmd.push_str(&format!(" -s '{}'", shell.as_str()));
    }
    if let Some(home) = &user.home {
        cmd.push_str(&format!(" -d '{}'", home.as_str()));
    }
    cmd.push_str(&format!(" {}", user.name.as_str()));
    cmd
}

/// Render Containerfile directives for users where `containerfile_strategy == "useradd"`.
///
/// Order: the update comment, group lines, `useradd` lines (each refused
/// user's FIXME in its place), `chpasswd`, the home file, the keys
/// comment. Nothing is written under `/home` or `/var` at build time.
pub fn render_containerfile_users(snap: &InspectionSnapshot) -> Vec<String> {
    let body = ContainerfileUserPlan::from_snapshot(snap).body();
    super::containerfile::section("Users and Groups", body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::users::UserGroupSection;

    /// Build a test snapshot with the given users and groups.
    fn snap_with(
        users: Vec<serde_json::Value>,
        groups: Vec<serde_json::Value>,
    ) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users,
            groups,
            ..Default::default()
        });
        snap
    }

    fn test_user() -> serde_json::Value {
        serde_json::json!({
            "name": "alice",
            "uid": 1001,
            "gid": 1001,
            "home": "/home/alice",
            "shell": "/bin/bash",
            "include": true,
            "containerfile_strategy": "useradd",
            "supplementary_groups": ["wheel", "docker"],
            "primary_group": "alice",
            "group_gids": {"alice": 1001, "wheel": 10, "docker": 1010},
            "password_hash": "$6$rounds=65536$salt$hash",
            "ssh_key_count": 1,
            "ssh_keys": ["ssh-rsa AAAA... alice@host"]
        })
    }

    fn test_group() -> serde_json::Value {
        serde_json::json!({
            "name": "docker",
            "gid": 1010,
            "source": "custom",
            "include": true
        })
    }

    // ----- Kickstart tests -----

    #[test]
    fn kickstart_renders_group_before_user() {
        let snap = snap_with(vec![test_user()], vec![test_group()]);
        let ks = render_kickstart(&snap);

        let group_pos = ks.find("group --name=docker").expect("group line missing");
        let user_pos = ks.find("user --name=alice").expect("user line missing");
        assert!(
            group_pos < user_pos,
            "group must come before user in kickstart"
        );

        // Verify flags
        assert!(ks.contains("--gid=1010"), "group gid missing");
        assert!(ks.contains("--uid=1001"), "user uid missing");
        assert!(ks.contains("--gid=1001"), "user gid missing");
        assert!(
            ks.contains("--groups=wheel,docker"),
            "supplementary groups missing"
        );
        assert!(
            ks.contains("--iscrypted --password="),
            "password flag missing"
        );
        assert!(
            ks.contains("sshkey --username=alice"),
            "sshkey line missing"
        );
    }

    #[test]
    fn kickstart_includes_shell_and_homedir() {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "alice", "uid": 1000, "gid": 1000,
                "shell": "/bin/bash", "home": "/home/alice",
                "supplementary_groups": []
            })],
            ..Default::default()
        });
        let ks = render_kickstart(&snap);
        assert!(ks.contains("--shell=/bin/bash"), "missing --shell: {ks}");
        assert!(
            ks.contains("--homedir=/home/alice"),
            "missing --homedir: {ks}"
        );
    }

    #[test]
    fn kickstart_empty_when_no_users() {
        let snap = InspectionSnapshot::new();
        assert!(render_kickstart(&snap).is_empty());
    }

    // ----- TOML tests -----

    #[test]
    fn toml_renders_group_and_user_blocks() {
        let snap = snap_with(vec![test_user()], vec![test_group()]);
        let toml = render_blueprint_toml(&snap);

        assert!(
            toml.contains("[[customizations.group]]"),
            "group block missing"
        );
        assert!(
            toml.contains("[[customizations.user]]"),
            "user block missing"
        );
        assert!(toml.contains("name = \"docker\""), "group name missing");
        assert!(toml.contains("gid = 1010"), "group gid missing");
        assert!(toml.contains("name = \"alice\""), "user name missing");
        assert!(toml.contains("uid = 1001"), "user uid missing");

        // Group block before user block
        let g_pos = toml.find("[[customizations.group]]").unwrap();
        let u_pos = toml.find("[[customizations.user]]").unwrap();
        assert!(g_pos < u_pos, "group block must precede user block");
    }

    #[test]
    fn toml_multi_key_uses_first_with_comment() {
        let user = serde_json::json!({
            "name": "bob",
            "uid": 1002,
            "gid": 1002,
            "include": true,
            "containerfile_strategy": "useradd",
            "ssh_keys": ["ssh-rsa KEY1 bob@a", "ssh-rsa KEY2 bob@b", "ssh-ed25519 KEY3 bob@c"]
        });
        let snap = snap_with(vec![user], vec![]);
        let toml = render_blueprint_toml(&snap);

        assert!(
            toml.contains("key = \"ssh-rsa KEY1 bob@a\""),
            "first key missing"
        );
        assert!(
            toml.contains("# NOTE: 2 additional SSH key(s)"),
            "multi-key comment missing"
        );
    }

    #[test]
    fn toml_empty_when_no_users() {
        let snap = InspectionSnapshot::new();
        assert!(render_blueprint_toml(&snap).is_empty());
    }

    // ----- Containerfile tests -----

    #[test]
    fn containerfile_useradd_with_groups_and_password() {
        let snap = snap_with(vec![test_user()], vec![test_group()]);
        let lines = render_containerfile_users(&snap);
        let output = lines.join("\n");

        // Every group the user names gets a line with the host's gid.
        for (group, gid) in [("alice", 1001), ("wheel", 10), ("docker", 1010)] {
            assert!(
                output.contains(&format!("groupadd -g {gid} {group} ||")),
                "{group}: no group line: {output}"
            );
        }
        assert!(
            output.contains("RUN useradd -M -u 1001 -g alice -G wheel,docker"),
            "useradd missing: {output}"
        );
        assert!(
            output.contains("WARNING: Embedding password hashes"),
            "password warning missing"
        );
        assert!(output.contains("chpasswd -e"), "chpasswd missing");
        // Keys are provisioned, not copied.
        assert!(!output.contains("COPY"), "{output}");
        assert!(!output.contains("install -d"), "{output}");
        assert!(
            output.contains("# alice had 1 key in /home/alice/.ssh/authorized_keys on the host."),
            "{output}"
        );
        assert!(output.contains(KEPT_KEYS_COMMENT), "{output}");

        let groupadd_pos = output.find("groupadd").unwrap();
        let useradd_pos = output.find("RUN useradd").unwrap();
        assert!(groupadd_pos < useradd_pos, "groupadd must precede useradd");
    }

    #[test]
    fn containerfile_useradd_creates_no_home_in_the_image() {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "bob", "uid": 1001, "gid": 1001,
                "shell": "/bin/bash", "home": "/home/bob",
                "containerfile_strategy": "useradd",
                "supplementary_groups": [],
                "primary_group": "bob",
                "group_gids": {"bob": 1001}
            })],
            groups: vec![serde_json::json!({"name": "bob", "gid": 1001, "source": "custom"})],
            ..Default::default()
        });
        let output = render_containerfile_users(&snap).join("\n");
        assert!(
            output.contains("RUN useradd -M -u 1001 -g bob -s '/bin/bash' -d '/home/bob' bob"),
            "{output}"
        );
        assert!(
            output.contains(
                "RUN printf '%s\\n' 'd /var/home/bob 0700 bob bob -' > /usr/lib/tmpfiles.d/inspectah-users.conf"
            ),
            "{output}"
        );
        assert!(!output.contains("useradd -m"), "{output}");
    }

    #[test]
    fn containerfile_skip_users_produces_empty() {
        let user = serde_json::json!({
            "name": "sysuser",
            "uid": 999,
            "gid": 999,
            "include": true,
            "containerfile_strategy": "skip"
        });
        let snap = snap_with(vec![user], vec![]);
        let lines = render_containerfile_users(&snap);
        assert!(lines.is_empty(), "skip strategy should produce no output");
    }

    #[test]
    fn containerfile_no_users_groups_produces_empty() {
        let snap = InspectionSnapshot::new();
        let lines = render_containerfile_users(&snap);
        assert!(lines.is_empty());
    }

    #[test]
    fn containerfile_only_emits_groups_for_useradd_users() {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![
                serde_json::json!({
                    "name": "alice", "uid": 1000, "gid": 1000,
                    "shell": "/bin/bash", "home": "/home/alice",
                    "containerfile_strategy": "useradd",
                    "supplementary_groups": ["wheel"],
                    "primary_group": "alice",
                    "group_gids": {"alice": 1000, "wheel": 10}
                }),
                serde_json::json!({
                    "name": "bob", "uid": 1001, "gid": 1001,
                    "shell": "/bin/bash", "home": "/home/bob",
                    "containerfile_strategy": "skip",
                    "supplementary_groups": [],
                    "primary_group": "bob",
                    "group_gids": {"bob": 1001}
                }),
            ],
            groups: vec![
                serde_json::json!({"name": "alice", "gid": 1000, "source": "custom"}),
                serde_json::json!({"name": "bob", "gid": 1001, "source": "custom"}),
            ],
            ..Default::default()
        });

        let output = render_containerfile_users(&snap).join("\n");
        assert!(
            output.contains("groupadd -g 1000 alice"),
            "alice's group needed for useradd: {output}"
        );
        assert!(
            !output.contains("groupadd -g 1001 bob"),
            "bob's group should NOT be emitted (skip strategy): {output}"
        );
    }

    /// Rows: (host home, whether the `/var` fragment has a rule for it,
    /// expected rule).
    #[test]
    fn home_rules_by_path() {
        let tmpfiles = |p: &str| HomeRule::Tmpfiles(ExportPath::parse(p).unwrap());
        let cases = [
            ("/home/alice", false, tmpfiles("/var/home/alice")),
            ("/home/staff/bob", false, tmpfiles("/var/home/staff/bob")),
            ("/home/alice/", false, tmpfiles("/var/home/alice")),
            ("/var/home/vic", false, tmpfiles("/var/home/vic")),
            ("/var/lib/svc", true, HomeRule::VarFragment),
            ("/var/opt/app", false, HomeRule::NotCreated),
            ("/opt/app", true, HomeRule::NotCreated),
            ("/home", false, HomeRule::NotCreated),
            ("/", false, HomeRule::Exists),
        ];
        for (home, rule, expected) in cases {
            let path = ExportPath::parse(home).unwrap();
            assert_eq!(home_rule(Some(&path), |_| rule), expected, "{home}");
        }
        assert_eq!(home_rule(None, |_| true), HomeRule::NotCreated);
    }

    /// The active (non-comment, non-blank) lines: what a build runs.
    fn active_lines(output: &str) -> Vec<&str> {
        output
            .lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .collect()
    }

    /// Every field on the group, `useradd` and `chpasswd` lines comes
    /// from the snapshot. One hostile field per case: it may cost that
    /// user or password a FIXME, but it never adds a command, and the
    /// ordinary user beside it is still created.
    #[test]
    fn hostile_user_fields_add_no_command() {
        use serde_json::json;
        let cases = [
            (
                "home",
                json!("/home/mallory; id > /tmp/inspectah-user-injection #"),
            ),
            ("shell", json!("/bin/sh; id #")),
            ("shell", json!("/bin/sh\nRUN id")),
            ("supplementary_groups", json!(["wheel;id"])),
            ("supplementary_groups", json!(["-g0"])),
            ("password_hash", json!("$6$salt$hash'; id; echo '")),
            ("password_hash", json!("$6$salt$hash\nRUN id")),
            ("password_hash", json!("$6$salt$ha:sh")),
            ("name", json!("-omallory")),
            ("name", json!("mall*ry")),
            ("primary_group", json!("mal'; id; '")),
            ("primary_group", json!("-g0")),
            ("group_gids", json!({"mallory": 1002, "x; id": 7})),
        ];
        for (field, value) in cases {
            let mut user = test_user();
            user["name"] = json!("mallory");
            user["uid"] = json!(1002);
            user["gid"] = json!(1002);
            user["primary_group"] = json!("mallory");
            user["group_gids"] = json!({"mallory": 1002, "wheel": 10, "docker": 1010});
            user[field] = value.clone();
            let snap = snap_with(vec![user, test_user()], vec![test_group()]);
            let cf = render_containerfile_users(&snap).join("\n");
            let active = active_lines(&cf);
            let what = format!("{field} = {value}");

            assert!(
                !active.iter().any(|l| l.contains("id >")
                    || l.contains("; id")
                    || l.contains("RUN id")
                    || l.contains("-g0")
                    || l.contains("-omallory")
                    // The group lines' own pattern is the only `*`.
                    || l.replace("[^:]*", "").contains('*')),
                "{what}: a hostile field reached a command: {active:#?}"
            );
            assert!(
                active
                    .iter()
                    .any(|l| l.starts_with("RUN useradd -M -u 1001 -g alice -G wheel,docker")),
                "{what}: the ordinary user was not created: {active:#?}"
            );
            assert!(cf.contains("# FIXME:"), "{what}: no FIXME: {cf}");
        }
    }
}
