use serde::Deserialize;

// Read secrets directly from file at compile time
const SECRETS_TOML: &str = include_str!("../../secrets.toml");

/// Defines the structure for the secrets.
#[derive(Deserialize, Debug, Clone)]
pub struct Secrets {
    /// Wi-Fi configuration.
    pub wifi: WiFiConfig,
    /// OpenWeather API configuration.
    pub openweather: OpenWeatherConfig,
    /// MQTT configuration.
    pub mqtt: MqttConfig,
}

/// Defines the structure for the Wi-Fi configuration.
#[derive(Deserialize, Debug, Clone)]
pub struct WiFiConfig {
    /// The SSID of the Wi-Fi network.
    pub ssid: String,
    /// The password of the Wi-Fi network.
    pub password: String,
}

/// Defines the structure for the OpenWeather API configuration.
#[derive(Deserialize, Debug, Clone)]
pub struct OpenWeatherConfig {
    /// The API key for the OpenWeather API.
    pub api_key: String,
    /// The city for which the weather should be displayed.
    pub city: String,
}

/// Defines the structure for the MQTT configuration.
#[derive(Deserialize, Debug, Clone)]
pub struct MqttConfig {
    /// The URL of the MQTT broker.
    pub broker_url: String,
    /// The username for the MQTT broker.
    pub mqtt_user: String,
    /// The password for the MQTT broker.
    pub mqtt_pw: String,
}

impl Secrets {
    /// Loads the secrets that were embedded at compile time.
    pub fn load() -> anyhow::Result<Self> {
        let secrets: Secrets = toml::from_str(SECRETS_TOML)
            .map_err(|e| anyhow::anyhow!("Error parsing secrets.toml: {}", e))?;
        Ok(secrets)
    }
}
