use std::fmt;

#[derive(Debug, Copy, Clone, PartialOrd, PartialEq)]
pub struct Date {
    pub y: u16,
    pub m: u8,
    pub d: u8,
}

impl Date {
    // takes only "2018-09-26"
    pub fn from_str(s: &str) -> Option<Self> {
        let mut parts = s.split(|c: char| !c.is_ascii_alphanumeric());
        let y: u16 = parts.next()?.parse().ok()?;
        let m: u8 = parts.next()?.parse().ok()?;
        let d: u8 = parts.next()?.parse().ok()?;
        Some(Date {y,m,d})
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}
