//! Provider and UUID namespace rules shared by the proxy, engine and client.
pub const PROPERTY: &str = "horizon:mixed-provider";
pub const ENGINE_ENV: &str = "HORIZON_MIXED_PROVIDERS";
pub const MOJANG_KEYS: &str = "https://api.minecraftservices.com/publickeys";
pub const ELY_TEXTURE_KEY: &str = "https://skinsystem.ely.by/signature-verification-key.der";

const VERSION_SHIFT: u32 = 76;
const VERSION_MASK: u128 = 15 << VERSION_SHIFT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Mojang,
    Ely,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mojang => "mojang",
            Self::Ely => "ely",
        }
    }

    pub fn verify_url(self) -> &'static str {
        match self {
            Self::Mojang => "https://sessionserver.mojang.com/session/minecraft/hasJoined",
            Self::Ely => "https://authserver.ely.by/session/hasJoined",
        }
    }

    pub fn name_url(self) -> &'static str {
        match self {
            Self::Mojang => "https://api.mojang.com/users/profiles/minecraft/",
            Self::Ely => "https://authserver.ely.by/api/users/profiles/minecraft/",
        }
    }

    pub fn profile_url(self) -> &'static str {
        match self {
            Self::Mojang => "https://sessionserver.mojang.com/session/minecraft/profile/",
            Self::Ely => "https://authserver.ely.by/session/profile/",
        }
    }

    pub fn effective(self, raw: u128) -> Option<u128> {
        if !rfc_variant(raw) {
            return None;
        }
        let tag = match (self, version(raw)) {
            (Self::Mojang, 4) => 4,
            (Self::Ely, 4) => 7,
            (Self::Ely, 5) => 8,
            _ => return None,
        };
        Some(with_version(raw, tag))
    }

    pub fn qualify(self, name: &str) -> String {
        format!("{}:{name}", self.label())
    }
}

fn rfc_variant(id: u128) -> bool {
    (id >> 62) & 3 == 2
}

fn version(id: u128) -> u128 {
    (id & VERSION_MASK) >> VERSION_SHIFT
}

fn with_version(id: u128, tag: u128) -> u128 {
    (id & !VERSION_MASK) | (tag << VERSION_SHIFT)
}

/// Provider and upstream UUID for an effective UUID in a mixed world.
pub fn upstream(id: u128) -> Option<(Provider, u128)> {
    if !rfc_variant(id) {
        return None;
    }
    match version(id) {
        4 => Some((Provider::Mojang, id)),
        7 => Some((Provider::Ely, with_version(id, 4))),
        8 => Some((Provider::Ely, with_version(id, 5))),
        _ => None,
    }
}

pub fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

pub fn provider_name(provider: Provider, name: &str) -> bool {
    valid_name(name) && (provider == Provider::Ely || !name.contains('-'))
}

pub fn target(text: &str) -> Option<(Provider, &str)> {
    let (prefix, name) = text.split_once(':')?;
    let provider = if prefix.eq_ignore_ascii_case("mojang") {
        Provider::Mojang
    } else if prefix.eq_ignore_ascii_case("ely") {
        Provider::Ely
    } else {
        return None;
    };
    provider_name(provider, name).then_some((provider, name))
}

pub fn parse_uuid(text: &str) -> Option<u128> {
    let bytes = text.as_bytes();
    match bytes.len() {
        32 if bytes.iter().all(u8::is_ascii_hexdigit) => u128::from_str_radix(text, 16).ok(),
        36 if bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        }) =>
        {
            u128::from_str_radix(&text.replace('-', ""), 16).ok()
        }
        _ => None,
    }
}

/// Local opt-in, independent of any profile received from a server.
pub fn client_enabled_for(address: &str) -> bool {
    client_enabled_for_with(address, |key| std::env::var(key).ok())
}

fn client_enabled_for_with(address: &str, var: impl Fn(&str) -> Option<String>) -> bool {
    if var("HORIZON_COSMETICS").as_deref() != Some("0") {
        return false;
    }
    let Some(configured) = var("HORIZON_MIXED_CLIENT") else {
        return false;
    };
    if !configured.contains(':') {
        return false;
    }
    match (
        crate::servers::parse_address(&configured),
        crate::servers::parse_address(address),
    ) {
        (Some((expected, port)), Some((host, actual_port))) => {
            !expected.is_empty() && port == actual_port && expected.eq_ignore_ascii_case(&host)
        }
        _ => false,
    }
}

/// Set explicitly by the supervisor on the private engine process.
pub fn engine_enabled() -> bool {
    std::env::var(ENGINE_ENV).as_deref() == Ok("1")
}

#[cfg(test)]
#[test]
fn reviewed_mixed_client_enablement_requires_address_and_cosmetics_off() {
    let enabled = |address, configured: Option<&str>, cosmetics: Option<&str>| {
        client_enabled_for_with(address, |key| match key {
            "HORIZON_MIXED_CLIENT" => configured.map(str::to_owned),
            "HORIZON_COSMETICS" => cosmetics.map(str::to_owned),
            _ => panic!("unexpected environment key {key}"),
        })
    };
    let deployment = Some("play.example:25565");
    assert!(enabled("PLAY.Example", deployment, Some("0")));
    assert!(enabled("play.example:25565", deployment, Some("0")));
    assert!(!enabled("play.example:25566", deployment, Some("0")));
    assert!(!enabled("other.example:25565", deployment, Some("0")));
    assert!(!enabled("play.example", deployment, Some("1")));
    assert!(!enabled("play.example", deployment, None));
    assert!(!enabled("play.example", None, Some("0")));
    assert!(!enabled("play.example", Some("play.example"), Some("0")));
    assert!(!enabled(
        "play.example",
        Some("play.example:bad"),
        Some("0")
    ));
}

#[test]
fn provider_namespaces_are_disjoint_and_reversible() {
    let raw = parse_uuid("12345678-1234-4234-8234-123456789abc").unwrap();
    let raw_v5 = with_version(raw, 5);
    let mojang = Provider::Mojang.effective(raw).unwrap();
    let ely_v4 = Provider::Ely.effective(raw).unwrap();
    let ely_v5 = Provider::Ely.effective(raw_v5).unwrap();
    assert_eq!(mojang, raw);
    assert_ne!(mojang, ely_v4);
    assert_ne!(ely_v4, ely_v5);
    assert_eq!(upstream(ely_v4), Some((Provider::Ely, raw)));
    assert_eq!(upstream(ely_v5), Some((Provider::Ely, raw_v5)));
    assert!(Provider::Mojang.effective(ely_v4).is_none());
    assert!(Provider::Ely.effective(ely_v5).is_none());
    assert!(Provider::Ely.effective(raw & !(3 << 62)).is_none());
    assert!(parse_uuid("1").is_none());
}
