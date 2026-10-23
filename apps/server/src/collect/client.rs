//! Who sent a collection request: client IP under proxy trust, geo headers or else the GeoIP
//! database, user agent (decision 0001, Proxy trust and headers; 0005, Operator IP block list;
//! 0018), or the visitor a trusted server integration names in its payload. Every value here is
//! transient request memory; none is stored or logged.

use std::net::IpAddr;

use axum::http::HeaderMap;
use axum::http::header::USER_AGENT;
use ipnet::IpNet;

use super::geoip::GeoIp;
use crate::config::CollectConfig;

/// Header names of one CDN. Coordinates come from the same set whose country header matched.
struct GeoHeaders {
    country: &'static str,
    region: &'static str,
    city: &'static str,
    /// Latitude and longitude. EdgeOne sends none.
    coordinates: Option<[&'static str; 2]>,
}

const GEO_HEADERS: [GeoHeaders; 4] = [
    GeoHeaders {
        country: "cf-ipcountry",
        region: "cf-region-code",
        city: "cf-ipcity",
        coordinates: Some(["cf-iplatitude", "cf-iplongitude"]),
    },
    GeoHeaders {
        country: "x-vercel-ip-country",
        region: "x-vercel-ip-country-region",
        city: "x-vercel-ip-city",
        coordinates: Some(["x-vercel-ip-latitude", "x-vercel-ip-longitude"]),
    },
    GeoHeaders {
        country: "cloudfront-viewer-country",
        region: "cloudfront-viewer-country-region",
        city: "cloudfront-viewer-city",
        coordinates: Some(["cloudfront-viewer-latitude", "cloudfront-viewer-longitude"]),
    },
    GeoHeaders {
        country: "eo-ipcountry",
        region: "eo-region-code",
        city: "eo-ipcity",
        coordinates: None,
    },
];

/// Location from trusted CDN headers or the GeoIP database (0001, 0017, 0018). Coordinates are
/// tenths of a degree.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Geo {
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub latitude: Option<i16>,
    pub longitude: Option<i16>,
}

/// The visitor's address: the peer, or the forwarded address when the peer is a trusted proxy.
pub fn visitor_ip(peer: IpAddr, headers: &HeaderMap, config: &CollectConfig) -> IpAddr {
    let peer = peer.to_canonical();
    if contains(&config.trusted_proxies, peer) {
        forwarded_ip(headers, config).unwrap_or(peer)
    } else {
        peer
    }
}

pub struct Client {
    /// `None` for a trusted server call without `payload.ip`.
    pub ip: Option<IpAddr>,
    /// Canonical text form, the identity input (0001). Empty without an address.
    pub ip_text: String,
    pub user_agent: String,
    pub geo: Geo,
}

impl Client {
    /// A trusted CDN country header decides the location; without one the GeoIP database looks
    /// up the address this rule picked (0018).
    pub fn resolve(
        peer: IpAddr,
        headers: &HeaderMap,
        config: &CollectConfig,
        geoip: &GeoIp,
    ) -> Self {
        let peer = peer.to_canonical();
        let trusted = contains(&config.trusted_proxies, peer);
        let ip = visitor_ip(peer, headers, config);
        let user_agent = headers
            .get(USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let from_cdn = if trusted {
            geo(headers)
        } else {
            Geo::default()
        };
        let geo = if from_cdn.country.is_some() {
            from_cdn
        } else {
            geoip.lookup(ip)
        };
        Self {
            ip: Some(ip),
            ip_text: ip.to_string(),
            user_agent,
            geo,
        }
    }

    /// The visitor of a request with a collection key: `payload.ip` and `payload.userAgent`,
    /// empty when absent, since the connection is the integration's server, not the visitor
    /// (0001 Missing inputs). No location: geo headers describe that server too. `None` when
    /// `ip` is not an address.
    pub fn from_payload(ip: Option<&str>, user_agent: Option<&str>) -> Option<Self> {
        let ip = match ip.map(str::trim).filter(|ip| !ip.is_empty()) {
            Some(text) => Some(parse_ip(text)?),
            None => None,
        };
        Some(Self {
            ip,
            ip_text: ip.map(|ip| ip.to_string()).unwrap_or_default(),
            user_agent: user_agent.unwrap_or_default().to_owned(),
            geo: Geo::default(),
        })
    }

    pub fn blocked(&self, config: &CollectConfig) -> bool {
        self.ip.is_some_and(|ip| contains(&config.ignore_ips, ip))
    }
}

fn contains(nets: &[IpNet], ip: IpAddr) -> bool {
    nets.iter().any(|net| net.contains(&ip))
}

/// Reads the configured client IP header. List headers are scanned right to left for the first
/// address that is not a trusted proxy.
fn forwarded_ip(headers: &HeaderMap, config: &CollectConfig) -> Option<IpAddr> {
    let name = config.client_ip_header.as_str();
    let values: Vec<&str> = headers
        .get_all(&config.client_ip_header)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    let candidates: Vec<IpAddr> = match name {
        "x-forwarded-for" => values
            .iter()
            .flat_map(|v| v.split(','))
            .filter_map(parse_ip)
            .collect(),
        "forwarded" => values
            .iter()
            .flat_map(|v| v.split([',', ';']))
            .filter_map(|part| {
                let (key, value) = part.trim().split_once('=')?;
                key.eq_ignore_ascii_case("for").then_some(value)
            })
            .filter_map(parse_ip)
            .collect(),
        _ => values
            .first()
            .and_then(|v| parse_ip(v))
            .into_iter()
            .collect(),
    };
    candidates
        .iter()
        .rev()
        .find(|ip| !contains(&config.trusted_proxies, **ip))
        .or(candidates.first())
        .copied()
}

/// `203.0.113.7`, `203.0.113.7:443`, `[2001:db8::1]:443`, `"[2001:db8::1]"`.
fn parse_ip(value: &str) -> Option<IpAddr> {
    let value = value.trim().trim_matches('"');
    if let Ok(ip) = value.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    let host = match value.strip_prefix('[') {
        Some(rest) => rest.split_once(']')?.0,
        None => value.rsplit_once(':')?.0,
    };
    host.parse::<IpAddr>().ok().map(|ip| ip.to_canonical())
}

fn geo(headers: &HeaderMap) -> Geo {
    let text = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|v| super::text::decode_component(v.trim()))
            .filter(|v| !v.is_empty())
    };
    for set in GEO_HEADERS {
        if let Some(country) = text(set.country) {
            let country: String = country.to_ascii_uppercase().chars().take(2).collect();
            let region = text(set.region).map(|region| region_code(&country, &region));
            let city = text(set.city).map(|city| city.chars().take(50).collect());
            let (latitude, longitude) = set
                .coordinates
                .and_then(|[lat, lng]| {
                    coordinates(text(lat)?.parse().ok()?, text(lng)?.parse().ok()?)
                })
                .unzip();
            return Geo {
                country: Some(country),
                region,
                city,
                latitude,
                longitude,
            };
        }
    }
    Geo::default()
}

/// `CC-RR` from a subdivision code with or without its country, at most 20 characters.
pub(super) fn region_code(country: &str, region: &str) -> String {
    let code = if region.contains('-') {
        region.to_owned()
    } else {
        format!("{country}-{region}")
    };
    code.chars().take(20).collect()
}

/// Latitude and longitude in degrees as tenths of a degree (about 11 km). Both or neither: a
/// value that is not a finite number in range drops the pair.
pub(super) fn coordinates(latitude: f64, longitude: f64) -> Option<(i16, i16)> {
    let tenths = |degrees: f64, limit: f64| {
        (degrees.is_finite() && degrees.abs() <= limit).then(|| (degrees * 10.0).round() as i16)
    };
    Some((tenths(latitude, 90.0)?, tenths(longitude, 180.0)?))
}

/// Browser, operating system and device class from a user agent, using the names upstream's
/// `detect-browser` and `ua-parser-js` produce for common clients. Unknown parts stay unset.
pub fn classify(
    user_agent: &str,
    screen: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    let ua = user_agent;
    let has = |needle: &str| ua.contains(needle);
    let browser = [
        ("EdgiOS", "edge-ios"),
        ("EdgA/", "edge-chromium"),
        ("Edg/", "edge-chromium"),
        ("Edge/", "edge"),
        ("SamsungBrowser", "samsung"),
        ("OPiOS", "opera"),
        ("Opera Mini", "opera-mini"),
        ("OPR/", "opera"),
        ("YaBrowser", "yandexbrowser"),
        ("FBAV", "facebook"),
        ("FBAN", "facebook"),
        ("Instagram", "instagram"),
        ("CriOS", "crios"),
        ("FxiOS", "fxios"),
        ("Firefox/", "firefox"),
        ("Chrome/", "chrome"),
        ("Chromium/", "chrome"),
        ("MSIE ", "ie"),
        ("Trident/", "ie"),
    ]
    .into_iter()
    .find(|(needle, _)| has(needle))
    .map(|(_, name)| name)
    .or_else(|| {
        let apple_mobile = has("iPhone") || has("iPad") || has("iPod");
        if has("Safari/") && has("Version/") {
            Some(if apple_mobile {
                "ios"
            } else if has("Android") {
                "android"
            } else {
                "safari"
            })
        } else {
            None
        }
    });

    let os = if has("iPhone") || has("iPad") || has("iPod") {
        Some("iOS")
    } else if has("Android") {
        Some("Android OS")
    } else if has("Windows Phone") {
        Some("Windows Mobile")
    } else if has("Windows NT 10.0") {
        Some("Windows 10")
    } else if has("Windows NT 6.3") {
        Some("Windows 8.1")
    } else if has("Windows NT 6.2") {
        Some("Windows 8")
    } else if has("Windows NT 6.1") {
        Some("Windows 7")
    } else if has("Windows NT 6.0") {
        Some("Windows Vista")
    } else if has("Windows NT 5.1") || has("Windows NT 5.2") {
        Some("Windows XP")
    } else if has("CrOS") {
        Some("Chrome OS")
    } else if has("Mac OS X") || has("Macintosh") {
        Some("Mac OS")
    } else if has("Linux") {
        Some("Linux")
    } else {
        None
    };

    let device = if has("iPad") || has("Tablet") || (has("Android") && !has("Mobile")) {
        "tablet"
    } else if has("Mobi") || has("iPhone") || has("iPod") || has("Android") {
        "mobile"
    } else if has("PlayStation") || has("Xbox") || has("Nintendo") {
        "console"
    } else if has("SmartTV") || has("SMART-TV") {
        "smarttv"
    } else {
        // ua-parser-js finds no device on desktops; upstream calls screens up to 1920 wide laptops.
        let width = screen
            .and_then(|s| s.split('x').next())
            .and_then(|w| w.trim().parse::<u32>().ok());
        match width {
            Some(w) if w <= 1920 => "laptop",
            _ => "desktop",
        }
    };
    (
        browser.map(str::to_owned),
        os.map(str::to_owned),
        (!user_agent.is_empty()).then(|| device.to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn config(trusted: &[&str]) -> CollectConfig {
        CollectConfig {
            trusted_proxies: trusted.iter().map(|n| n.parse().unwrap()).collect(),
            ..CollectConfig::default()
        }
    }

    /// MaxMind's City test database, which knows `81.2.69.142` and none of the documentation
    /// addresses the other tests use.
    fn geoip() -> GeoIp {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/geoip/GeoIP2-City-Test.mmdb");
        let config = CollectConfig {
            geoip_db: Some(fixture),
            ..CollectConfig::default()
        };
        GeoIp::open(std::path::Path::new("unused"), &config)
    }

    fn headers(xff: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_str(xff).unwrap());
        headers.insert("cf-ipcountry", HeaderValue::from_static("AT"));
        headers
    }

    #[test]
    fn untrusted_peers_cannot_choose_their_address_or_location() {
        let client = Client::resolve(
            "198.51.100.4".parse().unwrap(),
            &headers("203.0.113.7"),
            &config(&[]),
            &geoip(),
        );
        assert_eq!(client.ip_text, "198.51.100.4");
        assert_eq!(client.geo, Geo::default());
    }

    #[test]
    fn trusted_coordinates_are_rounded_to_tenths_of_a_degree() {
        let mut headers = HeaderMap::new();
        headers.insert("x-vercel-ip-country", HeaderValue::from_static("AT"));
        headers.insert("x-vercel-ip-city", HeaderValue::from_static("Vienna"));
        headers.insert("x-vercel-ip-latitude", HeaderValue::from_static("48.2082"));
        headers.insert("x-vercel-ip-longitude", HeaderValue::from_static("16.3738"));
        let client = Client::resolve(
            "127.0.0.1".parse().unwrap(),
            &headers,
            &config(&["127.0.0.1/32"]),
            &geoip(),
        );
        assert_eq!(client.geo.city.as_deref(), Some("Vienna"));
        assert_eq!(
            (client.geo.latitude, client.geo.longitude),
            (Some(482), Some(164))
        );
    }

    #[test]
    fn unusable_coordinates_are_dropped_as_a_pair() {
        let located = |lat: Option<&'static str>, lng: Option<&'static str>| {
            let mut headers = HeaderMap::new();
            headers.insert("cf-ipcountry", HeaderValue::from_static("AT"));
            for (name, value) in [("cf-iplatitude", lat), ("cf-iplongitude", lng)] {
                if let Some(value) = value {
                    headers.insert(name, HeaderValue::from_static(value));
                }
            }
            let geo = geo(&headers);
            assert_eq!(geo.country.as_deref(), Some("AT"));
            (geo.latitude, geo.longitude)
        };
        assert_eq!(
            located(Some("-33.9"), Some("-180")),
            (Some(-339), Some(-1800))
        );
        assert_eq!(located(Some("90.1"), Some("16.4")), (None, None));
        assert_eq!(located(Some("48.2"), Some("180.5")), (None, None));
        assert_eq!(located(Some("north"), Some("16.4")), (None, None));
        assert_eq!(located(Some("NaN"), Some("16.4")), (None, None));
        assert_eq!(located(Some("48.2"), None), (None, None));
    }

    #[test]
    fn a_trusted_cdn_header_wins_and_the_database_locates_everyone_else() {
        let geoip = geoip();
        let trusted = config(&["127.0.0.1/32"]);
        let london = "81.2.69.142";
        let proxy = "127.0.0.1".parse().unwrap();

        let client = Client::resolve(proxy, &headers(london), &trusted, &geoip);
        assert_eq!(
            client.geo,
            Geo {
                country: Some("AT".into()),
                ..Geo::default()
            }
        );

        let mut forwarded = HeaderMap::new();
        forwarded.insert("x-forwarded-for", HeaderValue::from_static("81.2.69.142"));
        let client = Client::resolve(proxy, &forwarded, &trusted, &geoip);
        assert_eq!(
            client.geo,
            Geo {
                country: Some("GB".into()),
                region: Some("GB-ENG".into()),
                city: Some("London".into()),
                latitude: Some(515),
                longitude: Some(-1),
            }
        );

        let client = Client::resolve(
            london.parse().unwrap(),
            &headers(london),
            &config(&[]),
            &geoip,
        );
        assert_eq!(client.geo.country.as_deref(), Some("GB"));

        let keyed = Client::from_payload(Some(london), None).unwrap();
        assert_eq!(keyed.geo, Geo::default());
    }

    #[test]
    fn trusted_proxies_are_skipped_right_to_left() {
        let config = config(&["10.0.0.0/8", "127.0.0.1/32"]);
        let client = Client::resolve(
            "127.0.0.1".parse().unwrap(),
            &headers("198.51.100.9, 203.0.113.7, 10.0.0.1"),
            &config,
            &geoip(),
        );
        assert_eq!(client.ip_text, "203.0.113.7");
        assert_eq!(client.geo.country.as_deref(), Some("AT"));
    }

    #[test]
    fn ipv4_mapped_addresses_are_unwrapped() {
        let client = Client::resolve(
            "::ffff:192.0.2.1".parse().unwrap(),
            &HeaderMap::new(),
            &config(&[]),
            &geoip(),
        );
        assert_eq!(client.ip_text, "192.0.2.1");
    }

    #[test]
    fn trusted_payload_addresses_are_canonical_and_may_be_absent() {
        let client = Client::from_payload(Some("::ffff:203.0.113.7"), Some("curl/8")).unwrap();
        assert_eq!(client.ip_text, "203.0.113.7");
        assert_eq!(client.user_agent, "curl/8");
        let client = Client::from_payload(None, None).unwrap();
        assert_eq!((client.ip, client.ip_text.as_str()), (None, ""));
        assert!(Client::from_payload(Some("not an address"), None).is_none());
    }

    #[test]
    fn common_user_agents_are_classified() {
        let chrome_mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
        assert_eq!(
            classify(chrome_mac, Some("1920x1080")),
            (
                Some("chrome".into()),
                Some("Mac OS".into()),
                Some("laptop".into())
            )
        );
        let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
        assert_eq!(
            classify(iphone, Some("390x844")),
            (
                Some("ios".into()),
                Some("iOS".into()),
                Some("mobile".into())
            )
        );
    }
}
