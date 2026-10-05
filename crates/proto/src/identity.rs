//! The client's hello (0x8A): who is connecting.
//!
//! EVIDENCE: the 2018 client's own bytes in the captured sessions (session-walk4,
//! frames 17, 40, 75, 97). Two forms:
//!   short (no character chosen yet): ... aa 04 09 02 01 <account> 03 05 10 00 <session>
//!                                    05 03 <83 first login / 03 after> 00 <tail>
//!   long  (a character chosen):      ... aa 04 0a 01 01 <character> 02 01 <account>
//!                                    03 05 10 00 <session> 05 03 03 00 <tail>
//! The server (experimental dsor/identity.py) reads the account 4 bytes before the
//! session anchor and the character from the long form's tag.
//! UNKNOWN: the meaning of the constant tail; it never changed across captures.

/// The launcher's identity: -accid and -sid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub account: u32,
    /// -sid, 16 bytes in the order the launcher prints them.
    pub session: [u8; 16],
}

impl Credentials {
    /// From the launcher's arguments: account number, session uuid text
    /// ("a6b4680e-391d-4e67-bc10-4b009c2245a5", dashes optional).
    pub fn parse(account: &str, session: &str) -> Option<Self> {
        let account = account.trim().parse().ok()?;
        let hex: String = session.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex.len() != 32 {
            return None;
        }
        let mut s = [0u8; 16];
        for (i, b) in s.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
        }
        Some(Self { account, session: s })
    }
}

const CLIENT_NAME: &str = "DrasaOnlineClient";
const TAIL: &str = "ffffffff8380e6800000078081000000080080000000088080000000090205803932b632b0b9b29698181880";

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// The 0x8A payload. `character` None is the short form; `first_login` marks the very
/// first connection of the run (the 0x83 discriminator the real client sends once).
pub fn hello(credentials: &Credentials, character: Option<u32>, first_login: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(104);
    out.push(0x8A);
    out.extend_from_slice(&(CLIENT_NAME.len() as u16).to_le_bytes());
    out.extend_from_slice(CLIENT_NAME.as_bytes());
    out.extend_from_slice(&[0xaa, 0x04]);
    match character {
        None => out.extend_from_slice(&[0x09, 0x02, 0x01]),
        Some(c) => {
            out.extend_from_slice(&[0x0a, 0x01, 0x01]);
            out.extend_from_slice(&c.to_le_bytes());
            out.extend_from_slice(&[0x02, 0x01]);
        }
    }
    out.extend_from_slice(&credentials.account.to_le_bytes());
    out.extend_from_slice(&[0x03, 0x05, 0x10, 0x00]);
    out.extend_from_slice(&credentials.session);
    out.extend_from_slice(&[0x05, 0x03, if first_login && character.is_none() { 0x83 } else { 0x03 }, 0x00]);
    out.extend_from_slice(&unhex(TAIL));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> Credentials {
        Credentials::parse("107909468", "a6b4680e-391d-4e67-bc10-4b009c2245a5").unwrap()
    }

    #[test]
    fn short_form_is_the_real_first_login() {
        let got = hello(&creds(), None, true);
        assert_eq!(got, unhex("8a110044726173614f6e6c696e65436c69656e74aa040902015c916e0603051000a6b4680e391d4e67bc104b009c2245a505038300ffffffff8380e6800000078081000000080080000000088080000000090205803932b632b0b9b29698181880"));
        assert_eq!(got.len(), 97);
    }

    #[test]
    fn long_form_names_the_character() {
        let got = hello(&creds(), Some(1), false);
        assert_eq!(got, unhex("8a110044726173614f6e6c696e65436c69656e74aa040a01010100000002015c916e0603051000a6b4680e391d4e67bc104b009c2245a505030300ffffffff8380e6800000078081000000080080000000088080000000090205803932b632b0b9b29698181880"));
        assert_eq!(got.len(), 103);
    }
}
