//! Messages in Brazilian Portuguese. Its own test binary (own process): the
//! language is global, and the unit tests run in English.

use ipsec_client_core::engine::EngineError;
use ipsec_client_core::i18n::{Lang, lang, set_lang};
use ipsec_client_core::profiles::CredentialKind;
use ipsec_client_core::secrets::describe;
use ipsec_client_core::session::FailureKind;

#[test]
fn messages_follow_the_chosen_language() {
    assert_eq!(lang(), Lang::En);
    assert_eq!(
        FailureKind::XauthCredentials.to_string(),
        "wrong XAuth username or password"
    );
    assert_eq!(describe(CredentialKind::XauthPassword), "XAuth password");

    set_lang(Lang::Pt);
    assert_eq!(
        FailureKind::XauthCredentials.to_string(),
        "usuário ou senha XAuth incorretos"
    );
    assert_eq!(describe(CredentialKind::XauthPassword), "senha XAuth");
    assert!(
        EngineError::HelperOutdated
            .to_string()
            .starts_with("o componente de rede instalado"),
        "{}",
        EngineError::HelperOutdated
    );

    set_lang(Lang::En);
    assert!(
        EngineError::HelperOutdated
            .to_string()
            .starts_with("the installed network component")
    );
}
