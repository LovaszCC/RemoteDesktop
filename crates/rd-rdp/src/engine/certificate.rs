//! The server certificate FreeRDP couldn't verify, as the store and the prompt need it. RDP servers
//! mostly present self-signed certificates; FreeRDP hands them over as PEM and they are checked against
//! the app's `rdp_known_hosts` (and the user) before any credential is sent.

use std::io;
use std::net::IpAddr;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use x509_cert::der::Decode as _;
use x509_cert::ext::pkix::SubjectAltName;
use x509_cert::ext::pkix::name::GeneralName;

/// What the server's certificate says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCertificate {
    /// The DER encoding.
    pub der: Vec<u8>,
    /// The subject, RFC 4514.
    pub subject: String,
    /// The issuer, RFC 4514.
    pub issuer: String,
    /// The common name, empty when the subject has none.
    pub common_name: String,
    /// DNS and IP subject alternative names, then the common name.
    pub names: Vec<String>,
}

impl ServerCertificate {
    /// Parses the first certificate of a PEM text.
    ///
    /// # Errors
    /// When it holds no X.509 certificate.
    pub fn from_pem(pem: &[u8]) -> io::Result<Self> {
        let text = std::str::from_utf8(pem).map_err(io::Error::other)?;
        let body = text
            .split("-----BEGIN CERTIFICATE-----")
            .nth(1)
            .and_then(|rest| rest.split("-----END CERTIFICATE-----").next())
            .ok_or_else(|| io::Error::other("no PEM certificate"))?;
        let base64: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        let der = STANDARD.decode(base64).map_err(io::Error::other)?;
        Self::parse(&der)
    }

    /// Parses a DER certificate.
    ///
    /// # Errors
    /// When it isn't an X.509 certificate.
    pub fn parse(der: &[u8]) -> io::Result<Self> {
        let cert = x509_cert::Certificate::from_der(der).map_err(io::Error::other)?;
        let tbs = &cert.tbs_certificate;
        let subject = tbs.subject.to_string();
        let common_name = common_name(&subject);
        let mut names = Vec::new();
        if let Ok(Some((_, SubjectAltName(general)))) = tbs.get::<SubjectAltName>() {
            for name in general {
                match name {
                    GeneralName::DnsName(dns) => names.push(dns.to_string()),
                    GeneralName::IpAddress(octets) => {
                        let bytes = octets.as_bytes();
                        let ip = <[u8; 4]>::try_from(bytes)
                            .map(IpAddr::from)
                            .or_else(|_| <[u8; 16]>::try_from(bytes).map(IpAddr::from));
                        if let Ok(ip) = ip {
                            names.push(ip.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        if !common_name.is_empty() {
            names.push(common_name.clone());
        }
        Ok(Self {
            der: der.to_vec(),
            subject,
            issuer: tbs.issuer.to_string(),
            common_name,
            names,
        })
    }
}

/// The `CN=` value of an RFC 4514 name (the most specific one, which comes first).
fn common_name(subject: &str) -> String {
    subject
        .split(',')
        .flat_map(|rdn| rdn.split('+'))
        .find_map(|attribute| {
            let (key, value) = attribute.trim().split_once('=')?;
            key.eq_ignore_ascii_case("CN").then(|| value.replace('\\', ""))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_name_is_taken_from_the_subject() {
        assert_eq!(common_name("CN=WIN-01"), "WIN-01");
        assert_eq!(common_name("CN=a.example.com,O=Example"), "a.example.com");
        assert_eq!(common_name("O=Example+cn=x"), "x");
        assert_eq!(common_name("O=Example"), "");
    }

    #[test]
    fn garbage_is_not_a_certificate() {
        assert!(ServerCertificate::parse(b"nope").is_err());
        assert!(ServerCertificate::from_pem(b"nope").is_err());
        assert!(
            ServerCertificate::from_pem(b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n")
                .is_err()
        );
    }
}
