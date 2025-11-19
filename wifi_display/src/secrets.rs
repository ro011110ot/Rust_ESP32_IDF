use serde::Deserialize;

// Secrets direkt aus Datei zur Compile-Zeit einlesen
const SECRETS_TOML: &str = include_str!("../../secrets.toml");

#[derive(Deserialize, Debug, Clone)]
pub struct Secrets {
    pub wifi: WiFiConfig,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WiFiConfig {
    pub ssid: String,
    pub password: String,
}

impl Secrets {
    /// Lädt Secrets die zur Compile-Zeit eingebettet wurden
    pub fn load() -> anyhow::Result<Self> {
        let secrets: Secrets = toml::from_str(SECRETS_TOML)
            .map_err(|e| anyhow::anyhow!("Fehler beim Parsen von secrets.toml: {}", e))?;
        Ok(secrets)
    }
}
