//! Typed core settings; unported and future settings are retained verbatim.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

macro_rules! choices {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $text)] $variant),+ }
    };
}
choices!(Aspect { Landscape => "landscape", Portrait => "portrait" });
choices!(Resolution { Hd => "1080p", UltraHd => "4k" });
choices!(RenderQuality { Standard => "standard", High => "high", Maximum => "maximum" });
choices!(RenderFormat { Mp4 => "mp4", Mov => "mov", Mkv => "mkv" });
choices!(RenderFps { Source => "source", Thirty => "30", Sixty => "60" });
choices!(AudioOutput { Beat => "beat", Guide => "guide" });

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub font: String,
    pub font_size: f64,
    pub aspect: Aspect,
    pub resolution: Resolution,
    pub render_quality: RenderQuality,
    pub render_format: RenderFormat,
    pub render_fps: RenderFps,
    pub audio_output: AudioOutput,
    pub audio_normalize: bool,
    pub timeline_linked: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            font: "Arial".into(),
            font_size: 72.,
            aspect: Aspect::Landscape,
            resolution: Resolution::Hd,
            render_quality: RenderQuality::High,
            render_format: RenderFormat::Mp4,
            render_fps: RenderFps::Source,
            audio_output: AudioOutput::Beat,
            audio_normalize: false,
            timeline_linked: true,
            extra: Map::new(),
        }
    }
}
impl Settings {
    /// Explicit Python-compatible normalization boundary for core settings.
    /// Parsing leaves unsupported settings in `extra` for subsequent ports.
    pub fn from_value(value: Value) -> Result<Self, String> {
        let Value::Object(mut object) = value else {
            return Err("Thiết lập phải là một đối tượng JSON".into());
        };
        if let Some(value) = object.get_mut("font_size") {
            let number = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
                .ok_or("font_size không hợp lệ")?;
            if !number.is_finite() {
                return Err("font_size không hữu hạn".into());
            }
            let rounded = (number.clamp(20., 240.) * 100.).round_ties_even() / 100.;
            *value = Value::from(rounded.round_ties_even());
        }
        let mut settings: Self =
            serde_json::from_value(Value::Object(object)).map_err(|e| e.to_string())?;
        settings.font = settings
            .font
            .chars()
            .filter(|c| !matches!(c, ',' | '\r' | '\n' | '{' | '}' | '\\'))
            .take(80)
            .collect::<String>()
            .trim()
            .to_owned();
        if settings.font.is_empty() {
            settings.font = "Arial".into();
        }
        Ok(settings)
    }
}
impl TryFrom<Value> for Settings {
    type Error = String;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        Self::from_value(value)
    }
}
pub(crate) fn deserialize_settings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Settings, D::Error> {
    Settings::from_value(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_unknown_fields_survive() {
        let settings = Settings::from_value(
            serde_json::json!({"font_size":110,"future":{"a":[1,2]},"title_enabled":true}),
        )
        .unwrap();
        assert_eq!(settings.font_size, 110.);
        assert_eq!(settings.audio_output, AudioOutput::Beat);
        let serialized = serde_json::to_value(settings).unwrap();
        assert_eq!(serialized["future"], serde_json::json!({"a":[1,2]}));
        assert_eq!(serialized["title_enabled"], true);
    }
    #[test]
    fn validates_core_and_matches_python_normalization() {
        let settings =
            Settings::from_value(serde_json::json!({"font":"{Arial},\n","font_size":"72.5"}))
                .unwrap();
        assert_eq!(settings.font, "Arial");
        assert_eq!(settings.font_size, 72.);
        assert_eq!(
            Settings::from_value(serde_json::json!({"font_size":999}))
                .unwrap()
                .font_size,
            240.
        );
        for value in [
            Value::Null,
            serde_json::json!({"aspect":"square"}),
            serde_json::json!({"audio_normalize":1}),
            serde_json::json!({"font_size":"NaN"}),
        ] {
            assert!(Settings::from_value(value).is_err());
        }
    }
}
