//! Asking a running service whether it can answer.
//!
//! It lives here rather than in the binary so a test can drive it against a
//! server started in-process: the container's health check is this, and a
//! check that is only ever run by hand is one nobody has seen fail.

use std::net::SocketAddr;

/// Fetch `/healthz` and report whether it answered 200.
///
/// The address is dialled as given, except that an unspecified one — which is
/// what a container binds — is reached on loopback, because `0.0.0.0` is a
/// listen address and not a destination.
pub async fn probe(bind: SocketAddr) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let address = reachable(bind);
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .map_err(|err| format!("cannot reach {address}: {err}"))?;
    let request = format!("GET /healthz HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|err| format!("cannot send to {address}: {err}"))?;
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .await
        .map_err(|err| format!("cannot read from {address}: {err}"))?;
    match answer.split_whitespace().nth(1) {
        Some("200") => Ok(()),
        _ => Err(format!("{address} answered {answer:?}")),
    }
}

/// The address to dial for one that may have been named as a listen address.
///
/// `0.0.0.0` is not a destination. Linux happens to accept a connection to it
/// and answer from the local host, so a check that skipped this would pass
/// there and fail on a system that refuses it — a difference the container's
/// own check cannot be allowed to inherit.
fn reachable(bind: SocketAddr) -> SocketAddr {
    let mut address = bind;
    if address.ip().is_unspecified() {
        address.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
    }
    address
}

#[cfg(test)]
mod tests {
    use super::reachable;

    #[test]
    fn a_wildcard_bind_is_dialled_on_loopback() {
        let mut bind = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
        assert_eq!(reachable(bind), "127.0.0.1:8080".parse().unwrap());
        // The IPv6 wildcard too, which is what a dual-stack bind names.
        bind = std::net::SocketAddr::from(([0u16; 8], 8080));
        assert_eq!(reachable(bind), "127.0.0.1:8080".parse().unwrap());
    }

    #[test]
    fn an_address_that_names_a_host_is_left_alone() {
        for written in ["127.0.0.1:8080", "192.0.2.7:9000", "[::1]:8080"] {
            let bind: std::net::SocketAddr = written.parse().unwrap();
            assert_eq!(reachable(bind), bind, "{written} was rewritten");
        }
    }
}
