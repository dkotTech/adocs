use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// Public address for absolute links in llms.txt and sitemap.xml. Without it the address
    /// is taken from the request `Host`.
    pub base_url: Option<String>,
    /// Read that address from `Forwarded` and `X-Forwarded-*`. Only behind a proxy that sets them.
    pub trust_forwarded: bool,
    /// Host names the service answers to, lowercase and without a port. Empty turns the check off.
    pub allowed_hosts: Vec<String>,
    /// Title in the header and in the index for AI agents.
    pub site_title: String,
    /// Mounted directory: a docker-compose volume, a Kubernetes persistent volume, S3 via a driver.
    pub data_dir: PathBuf,
    /// Archive file name inside data_dir: tar, tar.gz or zip.
    pub archive: String,
    /// Local directory the archive is copied into and where unpacked documents live.
    pub cache_dir: PathBuf,
    /// Size limit for the archive and its unpacked contents, a guard against archive bombs.
    pub max_total_bytes: u64,
    /// Whether the public search API accepts regular expressions.
    pub search_regex: bool,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// The host part of `host` or `host:port`; an IPv6 address keeps its brackets.
pub fn host_name(authority: &str) -> &str {
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            if host.starts_with('[') || !host.contains(':') {
                host
            } else {
                authority
            }
        }
        _ => authority,
    }
}

fn env_bool(key: &str) -> Result<bool, String> {
    match env_or(key, "").trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "" | "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("{key} must be true or false")),
    }
}

fn env_required(key: &str) -> Result<String, String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("variable {key} is not set"))
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let archive = env_required("ADOCS_ARCHIVE")?;
        // File name only: the config must not be able to escape the mounted directory.
        if archive.contains('/') || archive.contains('\\') || archive == "." || archive == ".." {
            return Err("ADOCS_ARCHIVE must be a file name inside ADOCS_DATA_DIR".into());
        }

        let max_mb: u64 = env_or("ADOCS_MAX_SIZE_MB", "1024")
            .parse()
            .map_err(|_| "ADOCS_MAX_SIZE_MB must be a number")?;

        let search_regex = env_bool("ADOCS_SEARCH_REGEX")?;

        let cache_dir = std::env::var("ADOCS_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("adocs"));

        Ok(Self {
            host: env_or("APP_HOST", "127.0.0.1"),
            port: env_or("APP_PORT", "8080")
                .parse()
                .map_err(|_| "APP_PORT must be a number")?,
            base_url: std::env::var("APP_BASE_URL")
                .ok()
                .map(|v| v.trim().trim_end_matches('/').to_string())
                .filter(|v| !v.is_empty()),
            trust_forwarded: env_bool("APP_TRUST_FORWARDED")?,
            allowed_hosts: env_or("APP_ALLOWED_HOSTS", "")
                .split(',')
                .map(|h| host_name(h.trim()).to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect(),
            site_title: env_or("SITE_TITLE", "adocs"),
            data_dir: PathBuf::from(env_required("ADOCS_DATA_DIR")?),
            archive,
            cache_dir,
            max_total_bytes: max_mb * 1024 * 1024,
            search_regex,
        })
    }
}
