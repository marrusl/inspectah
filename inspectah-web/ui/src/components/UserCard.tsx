import { useState, useCallback } from "react";
import { Label } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { UserDecision } from "../api/types";

export interface UserCardProps {
  user: UserDecision;
  /** SSH authorized_keys refs for this user (from section-level data). */
  sshRefs: Array<{ user: string; path: string }>;
  /** Whether subuid entries exist for this user. */
  hasSubuid: boolean;
  /** Whether sudoers rules mention this user. */
  hasSudo: boolean;
  isPending: boolean;
  onStrategyChange: (username: string, strategy: "skip" | "useradd") => void;
  onPasswordChange: (
    username: string,
    choice: "none" | "preserve" | "new",
    hash?: string,
  ) => void;
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
  sshRefs,
  hasSubuid,
  hasSudo,
  isPending,
  onStrategyChange,
  onPasswordChange,
}: UserCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [passwordExpanded, setPasswordExpanded] = useState(false);
  const [hashRevealed, setHashRevealed] = useState(false);
  const [newHash, setNewHash] = useState("");
  const [hashInputVisible, setHashInputVisible] = useState(
    user.password_choice === "new",
  );

  const isInteractive =
    user.classification === "human" || user.classification === "ambiguous";

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
        // Show the hash input but don't submit yet — wait for the hash.
        setHashInputVisible(true);
        return;
      }
      setHashInputVisible(false);
      onPasswordChange(user.name, choice);
    },
    [user.name, onPasswordChange],
  );

  const handleSetNewHash = useCallback(() => {
    if (newHash.trim()) {
      onPasswordChange(user.name, "new", newHash.trim());
      setNewHash("");
    }
  }, [user.name, newHash, onPasswordChange]);

  const classificationLabel =
    user.classification === "human"
      ? "Interactive user"
      : user.classification === "service"
        ? "Service account"
        : "Ambiguous classification";

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
    <div
      data-testid={`user-card-${user.name}`}
      style={cardStyle}
    >
      {/* Header row */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--sm)",
          flexWrap: "wrap",
        }}
      >
        <strong>{user.name}</strong>
        <span
          style={{
            fontSize: "var(--pf-t--global--font--size--sm)",
            opacity: 0.7,
          }}
        >
          UID {user.uid}
        </span>
        {hasSudo && <Label color="orange">sudo</Label>}
        {sshRefs.length > 0 && <Label color="blue">SSH keys</Label>}
        {hasSubuid && <Label color="teal">subuid</Label>}
        <Label
          color={
            user.classification === "human"
              ? "orange"
              : user.classification === "service"
                ? "grey"
                : "purple"
          }
        >
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
      </div>

      {!isInteractive && (
        <div
          style={{
            fontSize: "var(--pf-t--global--font--size--sm)",
            fontStyle: "italic",
            marginTop: "var(--pf-t--global--spacer--xs)",
          }}
        >
          Service account &mdash; review recommended before including
        </div>
      )}

      {/* Expanded details */}
      {expanded && (
        <div
          style={{
            marginTop: "var(--pf-t--global--spacer--sm)",
            paddingTop: "var(--pf-t--global--spacer--sm)",
            borderTop:
              "1px solid var(--pf-t--global--border--color--default)",
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
                        user.password_choice === "none" && !hashInputVisible
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
                          !hashInputVisible
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
                        hashInputVisible || user.password_choice === "new"
                      }
                      onChange={() => handlePasswordChoice("new")}
                      disabled={isPending}
                    />
                    Set new password
                  </label>
                  {/* Hash input — shown when "Set new password" is selected */}
                  {(hashInputVisible || user.password_choice === "new") && (
                    <div
                      style={{
                        marginLeft: "var(--pf-t--global--spacer--md)",
                        marginTop: "4px",
                      }}
                    >
                      <label
                        htmlFor={`new-hash-${user.name}`}
                        style={{
                          display: "block",
                          fontSize: "var(--pf-t--global--font--size--sm)",
                          fontWeight: 600,
                          marginBottom: "4px",
                        }}
                      >
                        Password hash (crypt(3) format)
                      </label>
                      <div
                        style={{
                          display: "flex",
                          gap: "var(--pf-t--global--spacer--xs)",
                        }}
                      >
                        <input
                          id={`new-hash-${user.name}`}
                          type="text"
                          placeholder="$6$salt$hash..."
                          value={newHash}
                          onChange={(e) => setNewHash(e.target.value)}
                          disabled={isPending}
                          style={{
                            flex: 1,
                            fontFamily: "monospace",
                            fontSize: "var(--pf-t--global--font--size--sm)",
                            padding: "2px 6px",
                          }}
                        />
                        <button
                          onClick={handleSetNewHash}
                          disabled={isPending || !newHash.trim()}
                          style={{
                            cursor: "pointer",
                            padding: "2px 8px",
                            fontSize: "var(--pf-t--global--font--size--sm)",
                          }}
                        >
                          Set
                        </button>
                      </div>
                      <div
                        style={{
                          fontSize: "var(--pf-t--global--font--size--xs)",
                          opacity: 0.7,
                          marginTop: "4px",
                        }}
                      >
                        Generate with:{" "}
                        <code>openssl passwd -6</code>
                      </div>
                    </div>
                  )}
                </fieldset>
              </div>
            )}
          </div>

          {/* SSH keys */}
          {sshRefs.length > 0 && (
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
              {sshRefs.map((ref) => (
                <div
                  key={ref.path}
                  style={{
                    fontSize: "var(--pf-t--global--font--size--sm)",
                    fontFamily: "monospace",
                  }}
                >
                  {ref.path}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
