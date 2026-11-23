use std::net::SocketAddr;
use tokio::net::UdpSocket;

pub const LOCAL_DNS_PORT: u16 = 15353;

/// Spawns a lightweight, zero-dependency UDP DNS responder on `127.0.0.1:15353`
/// that answers every `*.test` query with `127.0.0.1` (A) and `::1` (AAAA).
/// Paired with `/etc/resolver/test` on macOS, this gives instant wildcard `.test`
/// resolution with zero `/etc/hosts` edits and zero repeated admin prompts.
pub fn spawn_local_test_dns() {
    tauri::async_runtime::spawn(async {
        let addr: SocketAddr = ([127, 0, 0, 1], LOCAL_DNS_PORT).into();
        let Ok(socket) = UdpSocket::bind(addr).await else {
            return;
        };
        let mut buf = [0u8; 512];
        loop {
            let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                continue;
            };
            if let Some(resp) = build_dns_response(&buf[..len]) {
                let _ = socket.send_to(&resp, peer).await;
            }
        }
    });
}

/// Parses an RFC 1035 DNS query packet and constructs a response for `*.test` domains.
pub fn build_dns_response(query: &[u8]) -> Option<Vec<u8>> {
    // DNS header is 12 bytes minimum
    if query.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if qdcount == 0 {
        return None;
    }

    // Parse QNAME labels starting at offset 12
    let mut pos = 12usize;
    let mut labels: Vec<String> = Vec::new();
    while pos < query.len() {
        let label_len = query[pos] as usize;
        pos += 1;
        if label_len == 0 {
            break;
        }
        // Reject compressed pointers in question section or out-of-bounds labels
        if (label_len & 0xC0) != 0 || pos + label_len > query.len() {
            return None;
        }
        let label = std::str::from_utf8(&query[pos..pos + label_len]).ok()?;
        labels.push(label.to_ascii_lowercase());
        pos += label_len;
    }

    // QTYPE (2 bytes) + QCLASS (2 bytes)
    if pos + 4 > query.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([query[pos], query[pos + 1]]);
    let qclass = u16::from_be_bytes([query[pos + 2], query[pos + 3]]);
    let question_end = pos + 4;

    let is_test_domain = labels.last().map(|l| l == "test").unwrap_or(false) && labels.len() >= 2;
    let is_in_class = qclass == 1 || qclass == 255;

    let mut resp = Vec::with_capacity(question_end + 64);

    // Header (12 bytes)
    resp.extend_from_slice(&query[0..2]); // Transaction ID
    let rd = query[2] & 0x01; // Preserve Recursion Desired bit
    let flags_hi = 0x84 | rd; // QR=1 (response), AA=1 (authoritative)
    let rcode: u8 = if is_test_domain { 0 } else { 3 }; // NOERROR vs NXDOMAIN
    let flags_lo = rcode;
    resp.push(flags_hi);
    resp.push(flags_lo);
    resp.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1

    // Determine answers for *.test
    let answer_a = is_test_domain && is_in_class && (qtype == 1 || qtype == 255);
    let answer_aaaa = is_test_domain && is_in_class && (qtype == 28 || qtype == 255);
    let ancount: u16 = (answer_a as u16) + (answer_aaaa as u16);

    resp.extend_from_slice(&ancount.to_be_bytes()); // ANCOUNT
    resp.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    resp.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

    // Echo Question section verbatim
    resp.extend_from_slice(&query[12..question_end]);

    // Append A record (127.0.0.1) using name pointer 0xC00C -> offset 12
    if answer_a {
        resp.extend_from_slice(&[0xC0, 0x0C]); // Name pointer
        resp.extend_from_slice(&1u16.to_be_bytes()); // TYPE A
        resp.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN
        resp.extend_from_slice(&60u32.to_be_bytes()); // TTL 60s
        resp.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH 4
        resp.extend_from_slice(&[127, 0, 0, 1]);
    }

    // Append AAAA record (::1) using name pointer 0xC00C -> offset 12
    if answer_aaaa {
        resp.extend_from_slice(&[0xC0, 0x0C]); // Name pointer
        resp.extend_from_slice(&28u16.to_be_bytes()); // TYPE AAAA
        resp.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN
        resp.extend_from_slice(&60u32.to_be_bytes()); // TTL 60s
        resp.extend_from_slice(&16u16.to_be_bytes()); // RDLENGTH 16
        resp.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    }

    Some(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_query(domain: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![
            0x12, 0x34, // ID
            0x01, 0x00, // Standard query, RD=1
            0x00, 0x01, // QDCOUNT=1
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        for label in domain.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes()); // IN
        q
    }

    #[test]
    fn resolves_test_domains_to_loopback_and_rejects_others() {
        let q_a = make_query("phpmyadmin.test", 1);
        let r_a = build_dns_response(&q_a).expect("valid A response");
        assert_eq!(&r_a[0..2], &[0x12, 0x34]);
        assert_eq!(r_a[3] & 0x0F, 0); // NOERROR
        assert_eq!(u16::from_be_bytes([r_a[6], r_a[7]]), 1); // 1 answer
        assert_eq!(&r_a[r_a.len() - 4..], &[127, 0, 0, 1]);

        let q_aaaa = make_query("myapp.test", 28);
        let r_aaaa = build_dns_response(&q_aaaa).expect("valid AAAA response");
        assert_eq!(u16::from_be_bytes([r_aaaa[6], r_aaaa[7]]), 1);
        assert_eq!(r_aaaa[r_aaaa.len() - 1], 1);

        let q_https = make_query("myapp.test", 65);
        let r_https = build_dns_response(&q_https).expect("valid empty NOERROR");
        assert_eq!(r_https[3] & 0x0F, 0);
        assert_eq!(u16::from_be_bytes([r_https[6], r_https[7]]), 0);

        let q_com = make_query("example.com", 1);
        let r_com = build_dns_response(&q_com).expect("valid NXDOMAIN");
        assert_eq!(r_com[3] & 0x0F, 3); // NXDOMAIN
    }
}
