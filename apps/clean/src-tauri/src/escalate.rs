//! The privileged batch: the one place Spiral Clean asks for administrator
//! rights, and the only code that builds a string a root shell will execute.
//!
//! This is a **new trust boundary**, not a larger version of the removal one.
//! `remove.rs` guards *which files may be destroyed*; nothing here touches a
//! file. What this module guards is narrower and sharper: that the exact
//! command a user consented to is the exact command root runs.
//!
//! See ADR-0018 for why the batch is one `osascript` invocation, why a
//! privileged helper daemon was declined, and why the allowlist is a charset
//! rather than an escaper.

use serde::Serialize;

/// Emitted after every step so results can be attributed. Deliberately
/// alphanumeric — it must survive the same charset the tokens are held to.
const MARKER: &str = "SPIRALSTEP";

/// A single privileged action's work: one or more commands, run in order,
/// stopping at the first failure. `id` is the action it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegedStep {
    pub id: String,
    pub commands: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Succeeded,
    /// The command ran and reported a non-zero status.
    Failed(String),
    /// The batch never ran, so this step's state is unchanged.
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepResult {
    pub id: String,
    pub outcome: Outcome,
}

/// What happened to the batch as a whole.
pub enum BatchResult {
    Ran(Vec<StepResult>),
    /// The user dismissed the password prompt. Not an error — a decision.
    Cancelled,
    Failed(String),
}

// ---------------------------------------------------------------------------
// The guard
// ---------------------------------------------------------------------------

/// Whether a token may appear in a privileged command.
///
/// This is an **allowlist, and it is the whole security model of this
/// module.** Everything downstream — the single quoting, the AppleScript
/// literal, the `do shell script` call — is safe *because* nothing reaching
/// it can contain a character that means anything to a shell or to
/// AppleScript.
///
/// Deliberately not an escaper. Escaping is a function that must be correct
/// for every input; this is a predicate that must be correct for one small
/// set. Two escaping layers stacked — shell inside AppleScript — is where
/// this class of bug lives, and refusing the characters outright removes the
/// stack rather than reasoning about it.
///
/// Excluded on purpose and worth naming: `"` and `\` (AppleScript's own
/// escapes), `'` (the shell quote wrapped around every token), backtick,
/// `$`, `;`, `&`, `|`, `<`, `>`, `(`, `)`, newline, and every non-ASCII
/// character. `/` is admitted because absolute paths are unavoidable; it
/// carries no meaning to a shell inside a quoted token.
///
/// Per ADR-0012 this is proven by mutation: stub it to `true` and
/// `a_token_that_could_escape_the_quoting_is_refused` fails.
pub fn token_is_safe(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 512
        && token.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/' | ':' | '+' | '=')
        })
}

// ---------------------------------------------------------------------------
// Building the script
// ---------------------------------------------------------------------------

/// Wrap a validated token in single quotes.
///
/// Belt and braces: `token_is_safe` already refuses everything that would
/// need quoting, including `'` itself, so this cannot produce an unbalanced
/// quote. It stays because a token list that grows a space one day should
/// fail loudly at the guard rather than silently split into two arguments.
fn quote(token: &str) -> String {
    format!("'{token}'")
}

/// Assemble the shell body: each step's commands joined by `&&`, each step
/// followed by a marker carrying its index and exit status.
///
/// `&&` within a step so a step that fails halfway reports failure rather
/// than the status of a later command that happened to succeed. `;` between
/// steps so one failing step does not abort the rest — the same
/// no-single-failure-aborts-the-batch rule the removal flows already follow.
fn shell_body(steps: &[PrivilegedStep]) -> Result<String, String> {
    let mut body = String::new();
    for (index, step) in steps.iter().enumerate() {
        for command in &step.commands {
            if command.is_empty() {
                return Err(format!("{} has an empty command and was not run.", step.id));
            }
            for token in command {
                if !token_is_safe(token) {
                    return Err(format!(
                        "{} contains something Spiral Clean will not hand to an administrator shell, so nothing was run.",
                        step.id
                    ));
                }
            }
            if !body.ends_with("; ") && !body.is_empty() {
                body.push_str(" && ");
            }
            body.push_str(
                &command
                    .iter()
                    .map(|t| quote(t))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        body.push_str(&format!("; echo {MARKER}:{index}:$?; "));
    }
    Ok(body)
}

/// The complete AppleScript, ready for `osascript -e`.
///
/// The final assertion is the second half of the guard. The first refuses
/// dangerous characters in tokens; this one proves none reached the finished
/// script by any other route — a literal added here, a marker changed, a
/// future caller passing something that bypassed `shell_body`. It can only
/// fail if this module is edited wrongly, which is exactly when it is
/// wanted.
pub fn build_script(steps: &[PrivilegedStep]) -> Result<String, String> {
    let body = shell_body(steps)?;
    let script = format!("do shell script \"{body}\" with administrator privileges");

    if body.contains('"') || body.contains('\\') {
        return Err(
            "Spiral Clean built an administrator command it could not prove safe, so nothing was run."
                .to_string(),
        );
    }
    Ok(script)
}

// ---------------------------------------------------------------------------
// Running it
// ---------------------------------------------------------------------------

/// Run every privileged step behind **one** password prompt.
///
/// Returns `Cancelled` — not an error — when the user dismisses the prompt.
/// Declining to grant administrator rights is a legitimate answer, and
/// reporting it as a failure would be both wrong and alarming.
pub fn run(steps: &[PrivilegedStep]) -> BatchResult {
    if steps.is_empty() {
        return BatchResult::Ran(Vec::new());
    }
    let script = match build_script(steps) {
        Ok(script) => script,
        Err(message) => return BatchResult::Failed(message),
    };

    // Deliberately *not* `proc::output`. Every other tool this app runs
    // answers on its own; this one waits for a human to type a password, and
    // a deadline here would cancel the prompt out from under them.
    let output = match std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            return BatchResult::Failed(format!(
                "Could not ask for administrator access: {e}. Nothing was changed."
            ))
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if user_cancelled(&stderr) {
            return BatchResult::Cancelled;
        }
        return BatchResult::Failed(format!(
            "Administrator access was refused, so nothing was changed. macOS said: {}",
            stderr.trim()
        ));
    }

    BatchResult::Ran(attribute(steps, &String::from_utf8_lossy(&output.stdout)))
}

/// AppleScript reports a dismissed authorisation prompt as error -128, the
/// same code it uses for any user cancellation. The wording is localised;
/// the number is not, which is why the number is what is matched.
fn user_cancelled(stderr: &str) -> bool {
    stderr.contains("-128")
}

/// Match each step to the marker line it produced.
///
/// A step with no marker is reported `NotRun` rather than assumed to have
/// succeeded. Output can be truncated, and a silent step is the one case
/// where guessing would tell a user something was done when it was not.
///
/// **`do shell script` returns carriage-return-delimited output**, not
/// newline — an AppleScript convention that is easy to miss and would
/// otherwise make the whole result parse as one unmatched line.
fn attribute(steps: &[PrivilegedStep], stdout: &str) -> Vec<StepResult> {
    let statuses: Vec<(usize, i32)> = stdout
        .split(['\r', '\n'])
        .filter_map(|line| {
            let rest = line.trim().strip_prefix(MARKER)?.strip_prefix(':')?;
            let (index, status) = rest.split_once(':')?;
            Some((index.parse().ok()?, status.trim().parse().ok()?))
        })
        .collect();

    steps
        .iter()
        .enumerate()
        .map(|(index, step)| StepResult {
            id: step.id.clone(),
            outcome: match statuses.iter().find(|(i, _)| *i == index) {
                Some((_, 0)) => Outcome::Succeeded,
                Some((_, status)) => Outcome::Failed(format!("macOS reported error {status}.")),
                None => Outcome::NotRun,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, commands: &[&[&str]]) -> PrivilegedStep {
        PrivilegedStep {
            id: id.to_string(),
            commands: commands
                .iter()
                .map(|c| c.iter().map(|t| t.to_string()).collect())
                .collect(),
        }
    }

    // -- the guard, and its mutation proof ---------------------------------

    #[test]
    fn a_token_that_could_escape_the_quoting_is_refused() {
        // Stub `token_is_safe` to `true` and this test fails. Each of these
        // ends the single-quoted token and starts something else.
        assert!(!token_is_safe("'"));
        assert!(!token_is_safe("';rm -rf /;'"));
        assert!(!token_is_safe("foo'bar"));
    }

    #[test]
    fn a_token_that_could_escape_the_applescript_literal_is_refused() {
        assert!(!token_is_safe("\""));
        assert!(!token_is_safe("\\"));
        assert!(!token_is_safe("foo\"; do shell script \"evil"));
    }

    #[test]
    fn shell_metacharacters_are_refused() {
        for token in [
            "$(id)", "`id`", "a;b", "a&b", "a|b", "a>b", "a<b", "a b", "a\nb", "a\0b",
        ] {
            assert!(!token_is_safe(token), "{token} should be refused");
        }
    }

    #[test]
    fn non_ascii_is_refused() {
        // An allowlist refuses lookalikes without needing to know they exist.
        assert!(!token_is_safe("\u{2019}"));
        assert!(!token_is_safe("mdutil\u{00A0}-E"));
    }

    #[test]
    fn an_empty_or_oversized_token_is_refused() {
        assert!(!token_is_safe(""));
        assert!(!token_is_safe(&"a".repeat(513)));
    }

    #[test]
    fn the_tokens_the_real_actions_use_are_all_admitted() {
        for token in [
            "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister",
            "-kill", "-r", "-domain", "local", "system", "user",
            "mdutil", "-E", "/", "tmutil", "thinlocalsnapshots", "21474836480", "4",
            "dscacheutil", "-flushcache", "killall", "-HUP", "mDNSResponder",
            "ipconfig", "set", "en0", "DHCP", "pkill", "bluetoothd",
        ] {
            assert!(token_is_safe(token), "{token} is used by a real action");
        }
    }

    // -- script assembly ----------------------------------------------------

    #[test]
    fn a_refused_token_stops_the_whole_batch() {
        // Not "skip that step" — nothing runs. A batch the app cannot prove
        // safe is not one to run three quarters of.
        let steps = [
            step("ok", &[&["mdutil", "-E", "/"]]),
            step("bad", &[&["rm", "-rf /"]]),
        ];
        assert!(build_script(&steps).is_err());
    }

    #[test]
    fn the_finished_script_never_carries_a_quote_or_a_backslash() {
        let steps = [
            step(
                "dns",
                &[
                    &["dscacheutil", "-flushcache"],
                    &["killall", "-HUP", "mDNSResponder"],
                ],
            ),
            step("spotlight", &[&["mdutil", "-E", "/"]]),
        ];
        let script = build_script(&steps).unwrap();
        let body = script
            .strip_prefix("do shell script \"")
            .unwrap()
            .strip_suffix("\" with administrator privileges")
            .unwrap();
        assert!(
            !body.contains('"'),
            "an unescaped quote would end the AppleScript string"
        );
        assert!(!body.contains('\\'));
    }

    #[test]
    fn every_token_is_quoted_and_commands_within_a_step_are_chained() {
        let script = build_script(&[step(
            "dns",
            &[
                &["dscacheutil", "-flushcache"],
                &["killall", "-HUP", "mDNSResponder"],
            ],
        )])
        .unwrap();
        assert!(script.contains("'dscacheutil' '-flushcache' && 'killall' '-HUP' 'mDNSResponder'"));
    }

    #[test]
    fn steps_are_separated_so_one_failure_does_not_abort_the_rest() {
        let script = build_script(&[
            step("a", &[&["mdutil", "-E", "/"]]),
            step("b", &[&["pkill", "bluetoothd"]]),
        ])
        .unwrap();
        // `;` between steps, never `&&`.
        assert!(script.contains(&format!("echo {MARKER}:0:$?; 'pkill'")));
    }

    #[test]
    fn each_step_emits_a_marker_carrying_its_index() {
        let script = build_script(&[
            step("a", &[&["mdutil", "-E", "/"]]),
            step("b", &[&["pkill", "bluetoothd"]]),
        ])
        .unwrap();
        assert!(script.contains(&format!("echo {MARKER}:0:$?")));
        assert!(script.contains(&format!("echo {MARKER}:1:$?")));
    }

    #[test]
    fn an_empty_command_is_refused_rather_than_run_as_nothing() {
        assert!(build_script(&[step("empty", &[&[]])]).is_err());
    }

    #[test]
    fn an_empty_batch_never_prompts() {
        assert!(matches!(run(&[]), BatchResult::Ran(results) if results.is_empty()));
    }

    // -- attribution --------------------------------------------------------

    #[test]
    fn carriage_return_delimited_output_is_parsed() {
        // `do shell script` returns CR-delimited output. Splitting on \n
        // alone would leave every result unmatched and report the whole
        // batch as NotRun.
        let steps = [step("a", &[&["mdutil"]]), step("b", &[&["pkill"]])];
        let stdout = format!("{MARKER}:0:0\r{MARKER}:1:0\r");
        let results = attribute(&steps, &stdout);
        assert_eq!(results[0].outcome, Outcome::Succeeded);
        assert_eq!(results[1].outcome, Outcome::Succeeded);
    }

    #[test]
    fn newline_delimited_output_is_parsed_too() {
        let steps = [step("a", &[&["mdutil"]])];
        let results = attribute(&steps, &format!("{MARKER}:0:0\n"));
        assert_eq!(results[0].outcome, Outcome::Succeeded);
    }

    #[test]
    fn a_non_zero_status_is_reported_as_a_failure_for_that_step_only() {
        let steps = [step("a", &[&["mdutil"]]), step("b", &[&["pkill"]])];
        let results = attribute(&steps, &format!("{MARKER}:0:1\r{MARKER}:1:0\r"));
        assert!(matches!(results[0].outcome, Outcome::Failed(_)));
        assert_eq!(results[1].outcome, Outcome::Succeeded);
    }

    #[test]
    fn a_step_with_no_marker_is_not_run_rather_than_assumed_successful() {
        // Truncated output must never read as "done". This is the one place
        // guessing would tell a user something happened when it did not.
        let steps = [step("a", &[&["mdutil"]]), step("b", &[&["pkill"]])];
        let results = attribute(&steps, &format!("{MARKER}:0:0\r"));
        assert_eq!(results[0].outcome, Outcome::Succeeded);
        assert_eq!(results[1].outcome, Outcome::NotRun);
    }

    #[test]
    fn unrelated_command_output_is_ignored() {
        let steps = [step("a", &[&["mdutil"]])];
        let stdout = format!("Indexing enabled.\rsome other chatter\r{MARKER}:0:0\r");
        assert_eq!(attribute(&steps, &stdout)[0].outcome, Outcome::Succeeded);
    }

    #[test]
    fn a_malformed_marker_does_not_match_a_step() {
        let steps = [step("a", &[&["mdutil"]])];
        for stdout in [
            format!("{MARKER}:0"),
            format!("{MARKER}::0"),
            format!("{MARKER}:x:0"),
        ] {
            assert_eq!(attribute(&steps, &stdout)[0].outcome, Outcome::NotRun);
        }
    }

    #[test]
    fn every_step_gets_exactly_one_result_in_order() {
        let steps = [
            step("a", &[&["mdutil"]]),
            step("b", &[&["pkill"]]),
            step("c", &[&["ipconfig"]]),
        ];
        let results = attribute(&steps, "");
        assert_eq!(results.len(), 3);
        assert_eq!(
            results.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    // -- cancellation -------------------------------------------------------

    #[test]
    fn a_dismissed_prompt_is_recognised_by_its_error_number() {
        // The wording is localised; -128 is not.
        assert!(user_cancelled("execution error: User canceled. (-128)"));
        assert!(user_cancelled(
            "execution error: L\u{2019}utilisateur a annul\u{e9}. (-128)"
        ));
    }

    #[test]
    fn an_ordinary_failure_is_not_mistaken_for_a_cancellation() {
        assert!(!user_cancelled("execution error: something else (-1743)"));
        assert!(!user_cancelled(""));
    }
}
