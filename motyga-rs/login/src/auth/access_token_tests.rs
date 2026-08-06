use super::*;

#[test]
fn classifies_personal_access_tokens_by_prefix() {
    assert!(matches!(
        classify_motyga_access_token("at-example"),
        MotygaAccessToken::PersonalAccessToken("at-example")
    ));
    assert!(matches!(
        classify_motyga_access_token("header.payload.signature"),
        MotygaAccessToken::AgentIdentityJwt("header.payload.signature")
    ));
}
