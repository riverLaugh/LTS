#[allow(deprecated)]
use std::hash::{Hash, Hasher, SipHasher};

#[derive(Hash)]
struct CargoCompatibleSourceId<'a> {
    kind: Kind,
    url: &'a str,
}

#[derive(Hash)]
enum Kind {
    _1,
    _2,
    Registry,
}

#[allow(deprecated)]
pub fn short_hash(url: &str) -> String {
    let hashable = CargoCompatibleSourceId {
        url: url,
        kind: Kind::Registry,
    };
    let mut hasher = SipHasher::new_with_keys(0, 0);
    hashable.hash(&mut hasher);
    let num = hasher.finish();
    format!("{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        (num >> 0) as u8,
        (num >> 8) as u8,
        (num >> 16) as u8,
        (num >> 24) as u8,
        (num >> 32) as u8,
        (num >> 40) as u8,
        (num >> 48) as u8,
        (num >> 56) as u8,
    )
}

#[test]
fn hash() {
    assert_eq!("1ecc6299db9ec823", short_hash("https://github.com/rust-lang/crates.io-index"));
}
