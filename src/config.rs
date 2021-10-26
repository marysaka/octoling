use once_cell::sync::Lazy;
use serde::Deserialize;
use std::env;
use std::fs::File;
use std::io::Read;

pub const SHA256_SIZE: usize = 32;
pub const SERVER_VERSION: &str = "1.0.0";

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "github")]
    pub github_configs: Option<Vec<GithubConfig>>,
    #[serde(rename = "provider")]
    pub provider_configs: Option<Vec<ProviderConfig>>,
    #[serde(rename = "image")]
    pub image_configs: Option<Vec<ImageConfig>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GithubConfig {
    pub owner: String,
    pub repository: String,
    pub api_token: String,
    pub webhook_secret: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct GithubTokenResponse {
    pub token: String,
    pub expires_at: String,
}

impl GithubConfig {
    pub fn get_webhook_secret_slice(&self) -> &[u8] {
        self.webhook_secret.as_bytes()
    }

    pub fn get_repo_url(&self) -> String {
        format!("https://github.com/{}/{}/", self.owner, self.repository)
    }

    pub async fn request_new_repo_runner_token(&self) -> Option<String> {
        let request_url = format!(
            "https://api.github.com/repos/{}/{}/actions/runners/registration-token",
            self.owner, self.repository
        );
        let authorization_value = format!("Token {}", self.api_token);

        let response_result = reqwest::Client::new()
            .post(request_url.as_str())
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", authorization_value)
            .header("User-Agent", "octoling")
            .send()
            .await;

        if let Ok(response) = response_result {
            if let Ok(response_text) = response.text().await {
                if let Ok(token_response) =
                    serde_json::from_str::<GithubTokenResponse>(&response_text)
                {
                    return Some(token_response.token);
                }
            }
        }

        None
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub id: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ImageConfig {
    pub name: String,
    pub id: String,
    pub provider_id: String,
    pub enabled: bool,
    pub labels: Vec<String>,
}

static GLOBAL_CONFIG_PATH: Lazy<String> =
    Lazy::new(|| env::var("CONFIG_FILE").unwrap_or_else(|_| String::from("octoling.toml")));

pub static GLOBAL_CONFIG: Lazy<Config> = Lazy::new(|| {
    let mut file = File::open(GLOBAL_CONFIG_PATH.to_string()).unwrap();
    let mut config_str = String::new();

    file.read_to_string(&mut config_str).unwrap();

    toml::from_str(config_str.as_str()).unwrap()
});

pub static GLOBAL_GITHUB_CONFIG: Lazy<Vec<GithubConfig>> =
    Lazy::new(|| match &GLOBAL_CONFIG.github_configs {
        Some(github_configs) => github_configs.clone(),
        None => Vec::new(),
    });

pub static GLOBAL_PROVIDER_CONFIG: Lazy<Vec<ProviderConfig>> =
    Lazy::new(|| match &GLOBAL_CONFIG.provider_configs {
        Some(provider_configs) => provider_configs.clone(),
        None => Vec::new(),
    });

pub static GLOBAL_IMAGE_CONFIG: Lazy<Vec<ImageConfig>> =
    Lazy::new(|| match &GLOBAL_CONFIG.image_configs {
        Some(image_configs) => image_configs.clone(),
        None => Vec::new(),
    });

pub fn load() {
    Lazy::force(&GLOBAL_CONFIG_PATH);
    Lazy::force(&GLOBAL_CONFIG);
}

pub fn get_github_config_by_owner_and_repo(owner: &str, repository: &str) -> Option<GithubConfig> {
    for github_config in &*GLOBAL_GITHUB_CONFIG {
        if github_config.owner.as_str() == owner && github_config.repository.as_str() == repository
        {
            return Some(github_config.clone());
        }
    }

    None
}

pub fn get_image_config_by_label(label: &str) -> Option<ImageConfig> {
    for image_config in &*GLOBAL_IMAGE_CONFIG {
        for image_config_label in &image_config.labels {
            if image_config_label == label {
                return Some(image_config.clone());
            }
        }
    }

    None
}
