const PERSONAL_ACCESS_TOKEN_PREFIX: &str = "at-";

pub(super) enum MotygaAccessToken<'a> {
    PersonalAccessToken(&'a str),
    AgentIdentityJwt(&'a str),
}

pub(super) fn classify_motyga_access_token(access_token: &str) -> MotygaAccessToken<'_> {
    if access_token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX) {
        MotygaAccessToken::PersonalAccessToken(access_token)
    } else {
        MotygaAccessToken::AgentIdentityJwt(access_token)
    }
}

#[cfg(test)]
#[path = "access_token_tests.rs"]
mod tests;
