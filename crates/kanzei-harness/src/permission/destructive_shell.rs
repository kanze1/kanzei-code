//! Non-overridable protection for destructive shell primitives aimed at system roots.
//! This checks known shell primitives, not arbitrary program semantics. Untrusted code
//! still requires the isolated execution profile and its OS sandbox.
use super::{normalize_resource, BASH_ACTION};

pub(super) fn denied(action: &str, resource: &str) -> bool {
    if action != BASH_ACTION {
        return false;
    }
    let structured = serde_json::from_str::<serde_json::Value>(resource).ok();
    let command = structured
        .as_ref()
        .and_then(|v| v["command"].as_str())
        .unwrap_or(resource);
    commands(command).iter().any(|words| destructive(words))
}

fn destructive(words: &[String]) -> bool {
    let words = words.strip_prefix(&["sudo".to_owned()]).unwrap_or(words);
    let Some(program) = words.first() else {
        return false;
    };
    let program = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let args = &words[1..];
    match program.trim_end_matches(".exe") {
        "rm" | "rmdir" | "rd" | "remove-item" | "ri" => {
            args.iter().any(|arg| {
                let option = arg.to_ascii_lowercase();
                option == "--recursive"
                    || option == "-recurse"
                    || option == "/s"
                    || option.starts_with('-')
                        && !option.starts_with("--")
                        && option[1..].chars().all(|c| "rfdvi".contains(c))
                        && option[1..].contains('r')
            }) && args.iter().any(|arg| protected_target(arg))
        }
        // Raw filesystem/device writers are never ordinary workspace operations.
        "mkfs" | "mkfs.ext4" | "mkfs.xfs" | "format" | "diskpart" => true,
        "dd" => args.iter().any(|arg| arg.starts_with("of=/dev/")),
        _ => false,
    }
}

fn protected_target(target: &str) -> bool {
    if target == "*" {
        return false;
    }
    let target = target.trim_end_matches('*').trim_end_matches(['/', '\\']);
    if target.is_empty() {
        return true;
    }
    if matches!(
        target.to_ascii_lowercase().as_str(),
        "~" | "$home" | "${home}" | "%userprofile%" | "$env:userprofile"
    ) {
        return true;
    }
    let normalized = normalize_resource(target);
    normalized == "/"
        || normalized.len() == 2 && normalized.ends_with(':')
        || normalized.len() == 3 && normalized.as_bytes()[1] == b':' && normalized.ends_with('/')
}

// Preserve quoted whitespace and recognize shell statement boundaries. Quoted
// text following echo is data, whereas a new statement is checked independently.
fn commands(command: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for character in command.chars() {
        match character {
            '\'' | '"' if quote.is_none() => quote = Some(character),
            c if quote == Some(c) => quote = None,
            ';' | '&' | '|' | '\n' if quote.is_none() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
            }
            c if c.is_whitespace() && quote.is_none() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    if !words.is_empty() {
        commands.push(words);
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::{Effect, Ruleset};

    #[test]
    fn catastrophic_primitives_are_denied_even_with_global_allow_and_structured_resources() {
        let sets = [
            Ruleset::default(),
            Ruleset::new(vec![crate::rule("*", "*", Effect::Allow)]),
        ];
        for command in [
            "rm -rf /",
            "rm -fr '/'",
            "sudo rm --recursive --force /",
            "git status; rm -rf ~",
            "Remove-Item -Recurse -Force C:\\",
            "rd /s /q C:\\",
            "dd if=a of=/dev/sda",
            "format C:",
        ] {
            for rules in &sets {
                for resource in [
                    command.to_owned(),
                    serde_json::json!({"command":command,"workdir":"C:/project"}).to_string(),
                ] {
                    assert_eq!(
                        rules.evaluate("bash", &resource),
                        Effect::Deny,
                        "{resource}"
                    );
                    assert_eq!(rules.evaluate_with_rule("bash", &resource).0, Effect::Deny);
                }
            }
        }
        for command in [
            "rm -rf /tmp/build",
            "rm -rf build",
            "echo 'rm -rf /'",
            "git status",
        ] {
            assert!(!denied(BASH_ACTION, command), "{command}");
        }
    }
}
