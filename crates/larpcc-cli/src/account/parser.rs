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
