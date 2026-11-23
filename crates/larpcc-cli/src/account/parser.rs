use super::Account;

// A parser belongs to one reviewed row, never to a provider in general.
pub(crate) type Parser = fn(Option<i32>, &[u8], &[u8]) -> Account;

#[cfg(feature = "synthetic-launch")]
pub(crate) fn invented(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Account {
    if !stderr.is_empty() {
        return Account::Unknown;
    }
    match (code, stdout) {
        (Some(3), b"signed-out\n") => Account::SignedOut,
        (Some(0), _) => signed_in(stdout).unwrap_or(Account::Unknown),
        _ => Account::Unknown,
    }
}

#[cfg(feature = "synthetic-launch")]
fn signed_in(stdout: &[u8]) -> Option<Account> {
    let text = std::str::from_utf8(stdout).ok()?;
    let mut lines = text.strip_suffix('\n')?.split('\n');
    if lines.next()? != "signed-in" {
        return None;
    }
    let mut next = lines.next();
    let email = if let Some(email) = next.and_then(|line| line.strip_prefix("email=")) {
        let (local, domain) = email.split_once('@')?;
        if !valid_email_part(local, 64) || !valid_email_part(domain, 253) {
            return None;
        }
        next = lines.next();
        Some(email.to_owned())
    } else {
        None
    };
    let plan = if let Some(plan) = next.and_then(|line| line.strip_prefix("plan=")) {
        if plan.is_empty()
            || plan.len() > 64
            || plan.starts_with(' ')
            || plan.ends_with(' ')
            || !plan.bytes().all(|byte| (b' '..=b'~').contains(&byte))
        {
            return None;
        }
        next = lines.next();
        Some(plan.to_owned())
    } else {
        None
    };
    if next.is_some() {
        return None;
    }
    Some(Account::SignedIn { email, plan })
}

#[cfg(feature = "synthetic-launch")]
fn valid_email_part(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.bytes().all(|byte| byte.is_ascii_graphic() && byte != b'@')
}

#[cfg(all(test, feature = "synthetic-launch"))]
mod tests {
    use super::*;

    #[test]
    fn stage14_parser_accepts_only_the_rows_known_results() {
        let email = ["user", "@", "example.com"].concat();
        let plan = ["Pl", "us"].concat();
        for (fields, expected_email, expected_plan) in [
            (format!("email={email}\nplan={plan}\n"), Some(email.as_str()), Some(plan.as_str())),
            (format!("email={email}\n"), Some(email.as_str()), None),
            (format!("plan={plan}\n"), None, Some(plan.as_str())),
            (String::new(), None, None),
        ] {
            let output = format!("signed-in\n{fields}");
            match invented(Some(0), output.as_bytes(), b"") {
                Account::SignedIn { email, plan } => {
                    assert_eq!(email.as_deref(), expected_email);
                    assert_eq!(plan.as_deref(), expected_plan);
                }
                _ => panic!("fixture should parse as signed in"),
            }
        }
        assert!(matches!(invented(Some(3), b"signed-out\n", b""), Account::SignedOut));
    }

    #[test]
    fn stage14_parser_rejects_unrecognized_exit_stream_and_shape() {
        for (code, stdout, stderr) in [
            (None, b"signed-in\n".as_slice(), b"".as_slice()),
            (Some(3), b"signed-in\n", b""),
            (Some(0), b"signed-out\n", b""),
            (Some(0), b"signed-in\n", b"unexpected"),
            (Some(0), b"version=0.0.15\nsigned-in\n", b""),
            (Some(0), b"signed-in\nemail=a@b\nemail=x@y\n", b""),
            (Some(0), b"signed-in\nplan=Plus\nplan=Plus\n", b""),
            (Some(0), b"signed-in\nbackend=fixture\n", b""),
            (Some(0), b"signed-in\nplan=\xff\n", b""),
            (Some(0), b"signed-in\nplan=\x1b[31mPlus\n", b""),
            (Some(0), b"signed-in\nplan= Plus\n", b""),
            (Some(0), b"signed-in\nemail=a@@b\n", b""),
            (Some(0), b"signed-in\nemail=a b@c\n", b""),
            (Some(0), b"signed-in\nemail=@b\n", b""),
            (Some(0), b"signed-in\nplan=\n", b""),
            (Some(0), b"signed-in", b""),
        ] {
            assert!(matches!(invented(code, stdout, stderr), Account::Unknown));
        }
        for output in [
            format!("signed-in\nemail={}@b\n", "a".repeat(65)),
            format!("signed-in\nemail=a@{}\n", "b".repeat(254)),
            format!("signed-in\nplan={}\n", "p".repeat(65)),
        ] {
            assert!(matches!(invented(Some(0), output.as_bytes(), b""), Account::Unknown));
        }
    }
}
