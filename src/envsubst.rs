use regex::Regex;

pub fn expand(input: &str) -> String {
    let re = Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(:-([^}]*))?\}")
        .expect("envsubst regex should compile");
    re.replace_all(input, |captures: &regex::Captures| {
        let key = captures.get(1).expect("capture 1 must exist").as_str();
        let default = captures.get(3).map(|value| value.as_str()).unwrap_or("");
        std::env::var(key).unwrap_or_else(|_| default.to_string())
    })
    .into_owned()
}
