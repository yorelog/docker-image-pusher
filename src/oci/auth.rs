use reqwest::RequestBuilder;

#[derive(Clone, Debug)]
pub enum RegistryAuth {
    Anonymous,
    Basic { username: String, password: String },
}

impl RegistryAuth {
    pub fn anonymous() -> Self {
        RegistryAuth::Anonymous
    }

    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        RegistryAuth::Basic {
            username: username.into(),
            password: password.into(),
        }
    }

    pub fn apply(&self, req: RequestBuilder) -> RequestBuilder {
        match self {
            RegistryAuth::Anonymous => req,
            RegistryAuth::Basic { username, password } => req.basic_auth(username, Some(password)),
        }
    }
}
