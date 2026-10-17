use crate::{Shared, store::Account};
use std::time::Duration;
use url::Url;

/// Accept only an explicit SOCKS endpoint. Errors deliberately exclude the URL.
pub fn validate_proxy(value: &str) -> Result<Url, String> {
    if value.len() > 4096 || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Proxy URL must contain at most 4096 bytes without whitespace".into());
    }
    let url = Url::parse(value).map_err(|_| "Invalid SOCKS proxy URL")?;
    if !matches!(url.scheme(), "socks5" | "socks5h")
        || url.host_str().is_none_or(str::is_empty)
        || url.port().is_none_or(|port| port == 0)
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Use socks5://host:port or socks5h://host:port without a path, query, or fragment"
                .into(),
        );
    }
    if !url.username().is_empty() || url.password().is_some() {
        let user = credential_length(url.username())?;
        let pass = credential_length(url.password().unwrap_or_default())?;
        if !(1..=255).contains(&user) || !(1..=255).contains(&pass) {
            return Err(
                "SOCKS authentication requires a username and password of 1 to 255 bytes each"
                    .into(),
            );
        }
    }
    Ok(url)
}

fn credential_length(value: &str) -> Result<usize, String> {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut length = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err("Proxy credentials contain invalid percent encoding".into());
            }
            index += 3;
        } else {
            index += 1;
        }
        length += 1;
    }
    Ok(length)
}

pub fn proxy_display(value: &str) -> Result<String, String> {
    let url = validate_proxy(value)?;
    Ok(format!(
        "{}://{}",
        url.scheme(),
        &url[url::Position::BeforeHost..url::Position::AfterPort]
    ))
}

pub fn proxy_client(value: &str) -> Result<reqwest::Client, String> {
    let url = validate_proxy(value)?;
    let proxy = reqwest::Proxy::all(url).map_err(|_| "Invalid SOCKS proxy configuration")?;
    reqwest::Client::builder()
        .no_proxy()
        .proxy(proxy)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "Unable to initialize SOCKS proxy transport".into())
}

pub fn account_client(state: &Shared, account: &Account) -> Result<reqwest::Client, String> {
    if account.proxy_url.is_empty() {
        return Ok(state.client.clone());
    }
    let mut clients = state
        .proxy_clients
        .lock()
        .map_err(|_| "Proxy transport is unavailable")?;
    if let Some((saved, client)) = clients.get(&account.id)
        && saved == &account.proxy_url
    {
        return Ok(client.clone());
    }
    let value = state
        .store
        .unseal(&account.proxy_url)
        .map_err(|_| "Cannot decrypt account proxy")?;
    let client = proxy_client(&value)?;
    clients.insert(
        account.id.clone(),
        (account.proxy_url.clone(), client.clone()),
    );
    Ok(client)
}

/// Omitted/null preserves a saved proxy; an explicit empty string uses the default transport.
pub fn selected_client(
    state: &Shared,
    account: Option<&Account>,
    override_url: Option<&str>,
) -> Result<reqwest::Client, String> {
    match override_url {
        Some("") => Ok(state.client.clone()),
        Some(value) => proxy_client(value),
        None => account.map_or_else(|| Ok(state.client.clone()), |a| account_client(state, a)),
    }
}

pub fn connection_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "Proxy connection timed out"
    } else if error.is_connect() {
        "Proxy connection failed: check the address, credentials, DNS, and upstream TLS"
    } else {
        "Proxy request failed before an upstream HTTP response was received"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_socks_urls_without_echoing_credentials() {
        for value in [
            "socks5://127.0.0.1:1080",
            "socks5h://user:p%40ss@[::1]:1080/",
        ] {
            assert!(validate_proxy(value).is_ok(), "{value}");
        }
        for value in [
            "http://secret:password@localhost:1080",
            "socks5://localhost",
            "socks5://localhost:0",
            "socks5://localhost:1080/path",
            "socks5://localhost:1080?x=1",
            "socks5://localhost:1080#part",
            "socks5://user@localhost:1080",
            "socks5://user:p%xx@localhost:1080",
            "socks5://user:p ass@localhost:1080",
        ] {
            let error = validate_proxy(value).unwrap_err();
            assert!(!error.contains("secret") && !error.contains(value));
        }
        assert_eq!(
            proxy_display("socks5h://secret:password@[::1]:1080").unwrap(),
            "socks5h://[::1]:1080"
        );
    }
}
