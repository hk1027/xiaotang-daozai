use super::*;

#[test]
#[cfg(windows)]
fn transport_failures_distinguish_certificate_and_network_errors() {
    for (code, expected) in [
        (0x800b0109u32, "证书不受信任"),
        (0x800b010au32, "证书不受信任"),
        (0x80090325u32, "证书不受信任"),
        (0x800b0101u32, "证书不在有效期内"),
        (0x800b010fu32, "证书与服务器地址不匹配"),
        (0x800b010cu32, "证书已被吊销"),
        (10060, "连接超时"),
        (10061, "服务器拒绝连接"),
    ] {
        let error = std::io::Error::from_raw_os_error(code as i32);
        let failure = Failure::transport(&error);
        assert!(failure.message.contains(expected), "{code:#x}: {}", failure.message);
    }
}

#[test]
fn transport_failures_keep_typed_certificate_errors_and_hide_details() {
    use ureq::rustls::{CertificateError, Error};
    for (certificate, expected) in [
        (CertificateError::UnknownIssuer, "证书不受信任"),
        (CertificateError::Expired, "证书不在有效期内"),
        (CertificateError::NotValidForName, "证书与服务器地址不匹配"),
    ] {
        let error = std::io::Error::new(std::io::ErrorKind::InvalidData, Error::InvalidCertificate(certificate));
        let failure = Failure::transport(&error);
        assert!(failure.message.contains(expected), "{}", failure.message);
    }
    let error = std::io::Error::other("https://private.example/secret?token=synthetic-secret");
    assert!(!Failure::transport(&error).message.contains("synthetic-secret"));
}

#[test]
fn transport_failures_preserve_ureq_dns_and_nested_timeouts() {
    let client = ureq::AgentBuilder::new()
        .resolver(|_: &str| Err(std::io::Error::from(std::io::ErrorKind::NotFound)))
        .build();
    let error = client.get("https://synthetic.invalid/").call().unwrap_err();
    assert_eq!(Failure::transport(&error).kind, "dns");
    let timeout = std::io::Error::new(std::io::ErrorKind::Other,
        std::io::Error::from(std::io::ErrorKind::TimedOut));
    assert_eq!(Failure::transport(&timeout).kind, "timeout");
}

#[cfg(windows)]
mod tls {
    use super::*;
    use std::{io::{Read, Write}, net::TcpListener, sync::Arc};

    fn certificates() -> serde_json::Value {
        serde_json::from_str(include_str!("tls_test_certificates.json")).unwrap()
    }

    fn server(kind: &str) -> (String, std::thread::JoinHandle<usize>) {
        let certificates = certificates();
        let identity = native_tls::Identity::from_pkcs8(certificates[kind].as_str().unwrap().as_bytes(),
            certificates["key"].as_str().unwrap().as_bytes()).unwrap();
        let acceptor = native_tls::TlsAcceptor::new(identity).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("https://{}/", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
            let Ok(mut stream) = acceptor.accept(socket) else { return 0; };
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let Ok(n) = stream.read(&mut buffer) else { return 0; };
                if n == 0 { return 0; }
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end].lines().find_map(|line| line.split_once(':')
                        .filter(|(key, _)| key.eq_ignore_ascii_case("Content-Length"))
                        .and_then(|(_, value)| value.trim().parse::<usize>().ok())).unwrap_or(0);
                    if request.len() >= end + 4 + length { break; }
                }
            }
            assert!(String::from_utf8_lossy(&request).starts_with("PROPFIND / "));
            let xml = "<d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\"><d:response><d:href>/calendar/</d:href><d:resourcetype><c:calendar/></d:resourcetype></d:response></d:multistatus>";
            write!(stream, "HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{xml}", xml.len()).unwrap();
            1
        });
        (base, worker)
    }

    fn trusted_client() -> ureq::Agent {
        use base64::Engine;
        let root = base64::engine::general_purpose::STANDARD.decode(certificates()["root"].as_str().unwrap()).unwrap();
        let mut tls = native_tls::TlsConnector::builder();
        tls.add_root_certificate(native_tls::Certificate::from_der(&root).unwrap());
        ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).redirects(0)
            .tls_connector(Arc::new(tls.build().unwrap())).build()
    }

    #[test]
    fn calendar_agent_uses_system_tls_and_rejects_untrusted_certificate_before_http() {
        let (base, worker) = server("valid");
        let error = agent().unwrap().request("PROPFIND", &base).call().unwrap_err();
        assert!(std::error::Error::source(&error).unwrap().is::<native_tls::Error>(), "{error}");
        let failure = Failure::transport(&error);
        assert_eq!(failure.kind, "certificate_untrusted", "{}", failure.message);
        assert_eq!(worker.join().unwrap(), 0, "证书未被信任时不能发送 HTTP 请求");
    }

    #[test]
    fn trusted_private_ca_allows_caldav_discovery_and_keeps_certificate_checks() {
        for (kind, expected) in [("valid", None), ("expired", Some("certificate_validity")), ("hostname", Some("certificate_name"))] {
            let (base, worker) = server(kind);
            let cfg = Config { provider: Provider::Custom, username: "synthetic".into(), href: None, server: Some(base.clone()) };
            let result = discover_calendars(&trusted_client(), &cfg, "synthetic-password");
            let received = worker.join().unwrap();
            if let Some(expected) = expected {
                let failure = result.unwrap_err();
                assert_eq!(failure.kind, expected, "{}", failure.message);
                assert_eq!(received, 0);
            } else {
                assert_eq!(result.unwrap().0, [format!("{base}calendar/")]);
                assert_eq!(received, 1);
            }
        }
    }
}
