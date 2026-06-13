import { useState, useCallback } from "react";
import { Label } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { UserDecision } from "../api/types";
import { sha512Crypt } from "../utils/crypt";

export interface UserCardProps {
  user: UserDecision;
  isPending: boolean;
  onStrategyChange: (username: string, strategy: "skip" | "useradd") => void;
  onPasswordChange: (
    username: string,
    choice: "none" | "preserve" | "new",
    hash?: string,
  ) => Promise<void>;
}

/** Redact a crypt(3) hash, showing only the prefix. */
function redactHash(hash: string): string {
  // Match $id$salt$hash or similar patterns
  const match = hash.match(/^(\$[^$]+\$)/);
  if (match) return `${match[1]}<REDACTED>`;
  return "<REDACTED>";
}

export function UserCard({
  user,
  isPending,
  onStrategyChange,
  onPasswordChange,
}: UserCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [passwordExpanded, setPasswordExpanded] = useState(false);
  const [hashRevealed, setHashRevealed] = useState(false);
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [passwordError, setPasswordError] = useState("");
  const [isHashing, setIsHashing] = useState(false);
  const [passwordSet, setPasswordSet] = useState(
    user.password_choice === "new",
  );
  const [passwordInputVisible, setPasswordInputVisible] = useState(
    user.password_choice === "new",
  );

  const isInteractive = user.classification === "interactive";

  // "Preserve" is only available when the user has an existing password hash.
  const canPreserve = Boolean(user.password_hash);

  const handleStrategyChange = useCallback(
    (strategy: "skip" | "useradd") => {
      onStrategyChange(user.name, strategy);
    },
    [user.name, onStrategyChange],
  );

  const handlePasswordChoice = useCallback(
    (choice: "none" | "preserve" | "new") => {
      if (choice === "new") {
        setPasswordInputVisible(true);
        setPasswordSet(false);
        setNewPassword("");
        setConfirmPassword("");
        setPasswordError("");
        return;
      }
      setPasswordInputVisible(false);
      setPasswordSet(false);
      setNewPassword("");
      setConfirmPassword("");
      setPasswordError("");
      onPasswordChange(user.name, choice);
    },
    [user.name, onPasswordChange],
  );

  const handleSetPassword = useCallback(async () => {
    if (!newPassword) {
      setPasswordError("Password is required.");
      return;
    }
    if (newPassword !== confirmPassword) {
      setPasswordError("Passwords do not match.");
      return;
    }
    setPasswordError("");
    setIsHashing(true);
    try {
      const hash = await sha512Crypt(newPassword);
      await onPasswordChange(user.name, "new", hash);
      setPasswordSet(true);
      setNewPassword("");
      setConfirmPassword("");
    } catch {
      setPasswordError("Failed to set password. Please try again.");
    } finally {
      setIsHashing(false);
    }
  }, [user.name, newPassword, confirmPassword, onPasswordChange]);

  const classificationLabel = isInteractive
    ? "Interactive user"
    : "Non-interactive account";

  const cardStyle: React.CSSProperties = {
    borderLeft: isInteractive
      ? "3px solid var(--pf-t--global--color--status--warning--default)"
      : "3px solid var(--pf-t--global--color--status--info--default)",
    padding: "var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md)",
    marginBottom: "var(--pf-t--global--spacer--sm)",
    background: "var(--pf-t--global--background--color--secondary--default)",
    borderRadius: "var(--pf-t--global--border--radius--small)",
    opacity: !isInteractive ? 0.7 : 1,
  };

  return (
    <div data-testid={`user-card-${user.name}`} style={cardStyle}>
      {/* Header row */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--sm)",
          flexWrap: "wrap",
        }}
      >
        <input
          type="checkbox"
          checked={user.containerfile_strategy === "useradd"}
          onChange={() =>
            handleStrategyChange(
              user.containerfile_strategy === "useradd" ? "skip" : "useradd",
            )
          }
          disabled={isPending}
          aria-label={`Include ${user.name}`}
          style={{ flexShrink: 0 }}
        />
        <strong>{user.name}</strong>
        <span
          style={{
            fontSize: "var(--pf-t--global--font--size--sm)",
            opacity: 0.7,
          }}
        >
          UID {user.uid}
        </span>
        {user.has_sudo && <Label color="orange">sudo</Label>}
        {(user.ssh_key_count ?? 0) > 0 && (
          <Label color="blue">
            {user.ssh_key_count === 1
              ? "1 SSH key"
              : `${user.ssh_key_count} SSH keys`}
          </Label>
        )}
        <Label color={isInteractive ? "orange" : "grey"}>
          {classificationLabel}
        </Label>
        <div style={{ marginLeft: "auto", flexShrink: 0 }}>
          <button
            onClick={() => setExpanded((p) => !p)}
            aria-expanded={expanded}
            aria-label={`${expanded ? "Collapse" : "Expand"} ${user.name} details`}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: "4px",
              display: "flex",
              alignItems: "center",
            }}
          >
            {expanded ? <AngleDownIcon /> : <AngleRightIcon />}
          </button>
        </div>
      </div>

      {/* Detail line */}
      <div
        style={{
          fontSize: "var(--pf-t--global--font--size--sm)",
          opacity: 0.7,
          marginTop: "var(--pf-t--global--spacer--xs)",
        }}
      >
        {user.shell} &middot; {user.home}
        {user.supplementary_groups && user.supplementary_groups.length > 0 && (
          <> &middot; groups: {user.supplementary_groups.join(", ")}</>
        )}
      </div>

      {/* Classification rationale */}
      {user.classification_rationale && (
        <div
          style={{
            fontSize: "var(--pf-t--global--font--size--sm)",
            opacity: 0.7,
            marginTop: "var(--pf-t--global--spacer--xs)",
          }}
        >
          {user.classification_rationale}
        </div>
      )}

      {!isInteractive && (
        <div
          style={{
            fontSize: "var(--pf-t--global--font--size--sm)",
            fontStyle: "italic",
            marginTop: "var(--pf-t--global--spacer--xs)",
          }}
        >
          Non-interactive account &mdash; review recommended before including
        </div>
      )}

      {/* Expanded details */}
      {expanded && (
        <div
          style={{
            marginTop: "var(--pf-t--global--spacer--sm)",
            paddingTop: "var(--pf-t--global--spacer--sm)",
            borderTop: "1px solid var(--pf-t--global--border--color--default)",
          }}
        >
          {/* Strategy radio */}
          <fieldset
            style={{ border: "none", padding: 0, margin: 0 }}
            disabled={isPending}
          >
            <legend
              style={{
                fontWeight: 600,
                fontSize: "var(--pf-t--global--font--size--sm)",
                marginBottom: "var(--pf-t--global--spacer--xs)",
              }}
            >
              Containerfile strategy
            </legend>
            <label
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: "4px",
                marginRight: "var(--pf-t--global--spacer--md)",
                cursor: "pointer",
              }}
            >
              <input
                type="radio"
                name={`strategy-${user.name}`}
                value="skip"
                checked={user.containerfile_strategy === "skip"}
                onChange={() => handleStrategyChange("skip")}
                disabled={isPending}
              />
              Skip
            </label>
            <label
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: "4px",
                cursor: "pointer",
              }}
            >
              <input
                type="radio"
                name={`strategy-${user.name}`}
                value="useradd"
                checked={user.containerfile_strategy === "useradd"}
                onChange={() => handleStrategyChange("useradd")}
                disabled={isPending}
              />
              useradd
            </label>
          </fieldset>

          {/* Password section */}
          <div style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}>
            <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
              <button
                onClick={() => setPasswordExpanded((p) => !p)}
                style={{
                  background: "none",
                  border: "none",
                  cursor: "pointer",
                  padding: 0,
                  fontWeight: 600,
                  fontSize: "var(--pf-t--global--font--size--sm)",
                  display: "flex",
                  alignItems: "center",
                  gap: "4px",
                }}
              >
                {passwordExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
                Password options
              </button>
              {!passwordExpanded && (
                <span
                  style={{
                    fontSize: "var(--pf-t--global--font--size--xs)",
                    opacity: 0.6,
                    fontStyle: "italic",
                  }}
                >
                  {passwordSet || user.password_choice === "new"
                    ? "New password set"
                    : user.password_choice === "preserve"
                      ? "Existing password preserved"
                      : "No password"}
                </span>
              )}
            </div>
            {passwordExpanded && (
              <div
                style={{
                  marginTop: "var(--pf-t--global--spacer--xs)",
                  paddingLeft: "var(--pf-t--global--spacer--md)",
                }}
              >
                {user.password_hash && (
                  <div
                    style={{
                      marginBottom: "var(--pf-t--global--spacer--xs)",
                      fontSize: "var(--pf-t--global--font--size--sm)",
                    }}
                  >
                    Current hash:{" "}
                    <code>
                      {hashRevealed
                        ? user.password_hash
                        : redactHash(user.password_hash)}
                    </code>{" "}
                    <button
                      onClick={() => setHashRevealed((p) => !p)}
                      style={{
                        background: "none",
                        border: "none",
                        cursor: "pointer",
                        textDecoration: "underline",
                        fontSize: "inherit",
                        padding: 0,
                      }}
                    >
                      {hashRevealed ? "hide" : "reveal"}
                    </button>
                  </div>
                )}
                <fieldset
                  style={{ border: "none", padding: 0, margin: 0 }}
                  disabled={isPending}
                >
                  {/* No password — always available */}
                  <label
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "4px",
                      marginBottom: "4px",
                      cursor: "pointer",
                    }}
                  >
                    <input
                      type="radio"
                      name={`password-${user.name}`}
                      value="none"
                      checked={
                        user.password_choice === "none" && !passwordInputVisible
                      }
                      onChange={() => handlePasswordChoice("none")}
                      disabled={isPending}
                    />
                    No password
                  </label>
                  {/* Keep existing — only when user has a preserved hash */}
                  {canPreserve && (
                    <label
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: "4px",
                        marginBottom: "4px",
                        cursor: "pointer",
                      }}
                    >
                      <input
                        type="radio"
                        name={`password-${user.name}`}
                        value="preserve"
                        checked={
                          user.password_choice === "preserve" &&
                          !passwordInputVisible
                        }
                        onChange={() => handlePasswordChoice("preserve")}
                        disabled={isPending}
                      />
                      Keep existing password
                    </label>
                  )}
                  {/* Set new password — always available */}
                  <label
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "4px",
                      marginBottom: "4px",
                      cursor: "pointer",
                    }}
                  >
                    <input
                      type="radio"
                      name={`password-${user.name}`}
                      value="new"
                      checked={
                        passwordInputVisible || user.password_choice === "new"
                      }
                      onChange={() => handlePasswordChoice("new")}
                      disabled={isPending}
                    />
                    Set new password
                  </label>
                  {/* Password input — shown when "Set new password" is selected */}
                  {(passwordInputVisible || user.password_choice === "new") && (
                    <div
                      style={{
                        marginLeft: "var(--pf-t--global--spacer--md)",
                        marginTop: "4px",
                      }}
                    >
                      <label
                        htmlFor={`new-pw-${user.name}`}
                        style={{
                          display: "block",
                          fontSize: "var(--pf-t--global--font--size--sm)",
                          fontWeight: 600,
                          marginBottom: "4px",
                        }}
                      >
                        Password
                      </label>
                      <input
                        id={`new-pw-${user.name}`}
                        type="password"
                        value={newPassword}
                        onChange={(e) => {
                          setNewPassword(e.target.value);
                          setPasswordError("");
                        }}
                        disabled={isPending || isHashing}
                        style={{
                          width: "100%",
                          maxWidth: "300px",
                          fontSize: "var(--pf-t--global--font--size--sm)",
                          padding: "2px 6px",
                          marginBottom: "6px",
                        }}
                      />
                      <label
                        htmlFor={`confirm-pw-${user.name}`}
                        style={{
                          display: "block",
                          fontSize: "var(--pf-t--global--font--size--sm)",
                          fontWeight: 600,
                          marginBottom: "4px",
                        }}
                      >
                        Confirm password
                      </label>
                      <input
                        id={`confirm-pw-${user.name}`}
                        type="password"
                        value={confirmPassword}
                        onChange={(e) => {
                          setConfirmPassword(e.target.value);
                          setPasswordError("");
                        }}
                        disabled={isPending || isHashing}
                        style={{
                          width: "100%",
                          maxWidth: "300px",
                          fontSize: "var(--pf-t--global--font--size--sm)",
                          padding: "2px 6px",
                          marginBottom: "6px",
                        }}
                      />
                      {passwordError && (
                        <div
                          style={{
                            fontSize: "var(--pf-t--global--font--size--xs)",
                            color:
                              "var(--pf-t--global--color--status--danger--default)",
                            marginBottom: "6px",
                          }}
                        >
                          {passwordError}
                        </div>
                      )}
                      <button
                        onClick={handleSetPassword}
                        disabled={isPending || isHashing || !newPassword}
                        style={{
                          cursor: "pointer",
                          padding: "2px 8px",
                          fontSize: "var(--pf-t--global--font--size--sm)",
                        }}
                      >
                        {isHashing ? "Hashing..." : "Set password"}
                      </button>
                      <div
                        style={{
                          fontSize: "var(--pf-t--global--font--size--xs)",
                          opacity: 0.7,
                          marginTop: "6px",
                        }}
                      >
                        Password is hashed in your browser. The plaintext never
                        leaves this page.
                      </div>
                    </div>
                  )}
                </fieldset>
              </div>
            )}
          </div>

          {/* SSH keys */}
          {user.ssh_keys && user.ssh_keys.length > 0 && (
            <div style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}>
              <div
                style={{
                  fontWeight: 600,
                  fontSize: "var(--pf-t--global--font--size--sm)",
                  marginBottom: "var(--pf-t--global--spacer--xs)",
                }}
              >
                SSH authorized keys
              </div>
              {user.ssh_keys.map((key, idx) => (
                <div
                  key={idx}
                  style={{
                    fontSize: "var(--pf-t--global--font--size--sm)",
                    fontFamily: "monospace",
                    wordBreak: "break-all",
                  }}
                >
                  {key}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
