/// Returns whether `host` is one of the first-party Motyga hosts we are allowed
/// to treat as first-party traffic (e.g. for Cloudflare cookie affinity).
pub fn is_allowed_chatgpt_host(host: &str) -> bool {
    const EXACT_HOSTS: &[&str] = &["motyga.com"];
    const SUBDOMAIN_SUFFIXES: &[&str] = &[".motyga.com"];

    EXACT_HOSTS.contains(&host)
        || SUBDOMAIN_SUFFIXES
            .iter()
            .any(|suffix| host.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_motyga_hosts_without_suffix_tricks() {
        for host in [
            "motyga.com",
            "api.motyga.com",
            "ru.motyga.com",
            "foo.motyga.com",
        ] {
            assert!(is_allowed_chatgpt_host(host));
        }

        for host in [
            "evilmotyga.com",
            "motyga.com.evil.example",
            "api.openai.com",
            "chatgpt.com",
        ] {
            assert!(!is_allowed_chatgpt_host(host));
        }
    }
}
