//! Weather tool backed by Open-Meteo (no API key required).

use async_trait::async_trait;
use serde_json::Value;

use crate::error::{Error, Result};
use super::{Tool, ToolResult, ToolSchema};

pub struct Weather {
    pub http: reqwest::Client,
}

impl Default for Weather {
    fn default() -> Self { Self::new() }
}

impl Weather {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("http"),
        }
    }
}

#[async_trait]
impl Tool for Weather {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "weather".into(),
            description: "Get the current weather for a location (uses Open-Meteo, no API key).".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "latitude":{"type":"number"},
                    "longitude":{"type":"number"},
                    "label":{"type":"string","description":"Human label for the location"}
                },
                "required":["latitude","longitude"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let lat = args.get("latitude").and_then(Value::as_f64);
        let lon = args.get("longitude").and_then(Value::as_f64);
        let label = args
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("location")
            .to_string();
        let (Some(lat), Some(lon)) = (lat, lon) else {
            return Ok(ToolResult::err("latitude and longitude required"));
        };
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
             &current=temperature_2m,apparent_temperature,precipitation,weather_code,wind_speed_10m\
             &temperature_unit=fahrenheit&wind_speed_unit=mph&precipitation_unit=inch"
        );
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!("open-meteo HTTP {}", resp.status())));
        }
        let j: Value = resp.json().await?;
        let cur = j.get("current").cloned().unwrap_or(Value::Null);
        let temp = cur.get("temperature_2m").and_then(Value::as_f64).unwrap_or(f64::NAN);
        let feels = cur.get("apparent_temperature").and_then(Value::as_f64).unwrap_or(f64::NAN);
        let precip = cur.get("precipitation").and_then(Value::as_f64).unwrap_or(0.0);
        let wind = cur.get("wind_speed_10m").and_then(Value::as_f64).unwrap_or(0.0);
        let code = cur.get("weather_code").and_then(Value::as_i64).unwrap_or(-1);
        let text = format!(
            "{label}: {temp:.0}°F (feels {feels:.0}°F), wind {wind:.0} mph, precip {precip:.2}\" (wmo {code})"
        );
        Ok(ToolResult::ok_text(text).with_structured(j))
    }
}
