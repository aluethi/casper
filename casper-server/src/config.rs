use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub listen: ListenConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub auth: AuthConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListenConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_main_pool_size")]
    pub main_pool_size: u32,
    #[serde(default = "default_analytics_pool_size")]
    pub analytics_pool_size: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub signing_key_file: Option<String>,
    pub master_key_file: Option<String>,
    #[serde(default)]
    pub dev_auth: bool,
    pub admin_email: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            signing_key_file: None,
            master_key_file: None,
            dev_auth: true,
            admin_email: None,
        }
    }
}

fn default_port() -> u16 {
    3000
}
fn default_main_pool_size() -> u32 {
    30
}
fn default_analytics_pool_size() -> u32 {
    10
}

impl ServerConfig {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}
