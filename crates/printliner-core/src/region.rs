//! The Bambu cloud regions. A global account uses `api.bambulab.com` and
//! either the US or the EU MQTT broker; a China account has its own hosts.

/// The cloud MQTT brokers' port (MQTT over TLS).
pub const MQTT_PORT: u16 = 8883;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Region {
    /// `api.bambulab.com` and the US broker.
    #[default]
    Global,
    /// `api.bambulab.com` and the EU broker.
    Europe,
    /// `api.bambulab.cn` and the China broker.
    China,
}

impl Region {
    pub const ALL: [Region; 3] = [Region::Global, Region::Europe, Region::China];

    /// The id saved in `settings.json`.
    pub fn id(self) -> &'static str {
        match self {
            Region::Global => "global",
            Region::Europe => "europe",
            Region::China => "china",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Region::Global => "Global",
            Region::Europe => "Europe",
            Region::China => "China",
        }
    }

    pub fn parse(id: &str) -> Option<Region> {
        Region::ALL.into_iter().find(|r| r.id() == id)
    }

    pub fn api_base(self) -> &'static str {
        match self {
            Region::China => "https://api.bambulab.cn",
            _ => "https://api.bambulab.com",
        }
    }

    pub fn mqtt_host(self) -> &'static str {
        match self {
            Region::Global => "us.mqtt.bambulab.com",
            Region::Europe => "eu.mqtt.bambulab.com",
            Region::China => "cn.mqtt.bambulab.com",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for r in Region::ALL {
            assert_eq!(Region::parse(r.id()), Some(r));
        }
        assert_eq!(Region::parse("mars"), None);
    }

    #[test]
    fn hosts() {
        assert_eq!(Region::Europe.api_base(), "https://api.bambulab.com");
        assert_eq!(Region::Europe.mqtt_host(), "eu.mqtt.bambulab.com");
        assert_eq!(Region::China.api_base(), "https://api.bambulab.cn");
    }
}
