use super::*;

#[test]
fn apple_is_the_default_and_ollama_names_its_model_as_the_source() {
    let settings = Settings::default();
    assert_eq!(settings.provider, Provider::Apple);
    assert_eq!(settings.source(), "apple");
    let ollama = Settings {
        provider: Provider::Ollama,
        model: " llama3.2 ".into(),
        ..settings
    };
    assert_eq!(ollama.source(), "ollama:llama3.2");
}

#[test]
fn addresses_are_trimmed_and_must_be_http() {
    let normalized = clean(Settings {
        provider: Provider::Ollama,
        url: " http://localhost:11434/ ".into(),
        model: " llava ".into(),
    })
    .unwrap();
    assert_eq!(normalized.url, "http://localhost:11434");
    assert_eq!(normalized.model, "llava");
    assert!(clean(Settings {
        provider: Provider::Ollama,
        url: "localhost:11434".into(),
        model: "llava".into(),
    })
    .is_err());
}
