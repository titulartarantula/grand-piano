//! Minimal SFZ parser covering the opcodes used by the Salamander Grand Piano
//! (FreePats SFZ+FLAC V3+20200602). Unknown opcodes are kept but ignored.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Attack,
    Release,
}

/// What a region is for, derived from its trigger and opcodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    /// A normal note sample.
    Note,
    /// String resonance heard when the dampers fall back on the strings.
    DamperRelease,
    /// Mechanical key-release noise (no pitch tracking), heard on every key-up.
    KeyNoise,
    /// Sustain pedal going down.
    PedalDown,
    /// Sustain pedal coming up.
    PedalUp,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub sample: String,
    pub lokey: u8,
    pub hikey: u8,
    pub lovel: u8,
    pub hivel: u8,
    pub keycenter: u8,
    pub pitch_keytrack: f32,
    pub volume_db: f32,
    pub amp_veltrack: f32,
    pub ampeg_release: f32,
    pub rt_decay: f32,
    pub trigger: Trigger,
    pub lorand: f32,
    pub hirand: f32,
    pub kind: RegionKind,
}

fn parse_key(v: &str) -> Option<i32> {
    if let Ok(n) = v.parse::<i32>() {
        return Some(n);
    }
    // Note names like c4, f#3 (SFZ: c4 = 60)
    let v = v.to_ascii_lowercase();
    let mut chars = v.chars();
    let base = match chars.next()? {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (acc, oct) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest.as_str())
    };
    let oct: i32 = oct.parse().ok()?;
    Some((oct + 1) * 12 + base + acc)
}

/// Splits one line's content into opcode=value pairs. Values may contain spaces
/// only for `sample=` (paths), which runs until the next ` word=` token.
fn opcodes(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if let Some((k, v)) = t.split_once('=') {
            let mut value = v.to_string();
            if k == "sample" {
                while i + 1 < tokens.len() && !tokens[i + 1].contains('=') && !tokens[i + 1].starts_with('<') {
                    i += 1;
                    value.push(' ');
                    value.push_str(tokens[i]);
                }
            }
            out.push((k.to_string(), value));
        }
        i += 1;
    }
    out
}

pub fn parse(text: &str) -> anyhow::Result<Vec<Region>> {
    let mut group: HashMap<String, String> = HashMap::new();
    let mut regions = Vec::new();
    for raw in text.lines() {
        let line = match raw.find("//") {
            Some(p) => &raw[..p],
            None => raw,
        };
        let mut rest = line.trim();
        while !rest.is_empty() {
            if let Some(stripped) = rest.strip_prefix('<') {
                let end = stripped.find('>').ok_or_else(|| anyhow::anyhow!("bad header: {raw}"))?;
                let header = &stripped[..end];
                rest = stripped[end + 1..].trim_start();
                // opcodes until next header on the same line
                let next = rest.find('<').unwrap_or(rest.len());
                let ops = opcodes(&rest[..next]);
                rest = rest[next..].trim_start();
                match header {
                    "group" | "global" | "master" | "control" => {
                        if header == "group" {
                            group.clear();
                        }
                        for (k, v) in ops {
                            group.insert(k, v);
                        }
                    }
                    "region" => {
                        let mut m = group.clone();
                        for (k, v) in ops {
                            m.insert(k, v);
                        }
                        if let Some(r) = build_region(&m)? {
                            regions.push(r);
                        }
                    }
                    _ => {}
                }
            } else {
                // continuation opcodes for the most recent header (rare; not in Salamander)
                break;
            }
        }
    }
    Ok(regions)
}

fn build_region(m: &HashMap<String, String>) -> anyhow::Result<Option<Region>> {
    let Some(sample) = m.get("sample") else { return Ok(None) };
    let f = |k: &str, d: f32| m.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
    let key = |k: &str| m.get(k).and_then(|v| parse_key(v));
    let (lokey, hikey) = if let Some(k) = key("key") {
        (k, k)
    } else {
        (key("lokey").unwrap_or(0), key("hikey").unwrap_or(127))
    };
    let keycenter = key("pitch_keycenter").or(key("key")).unwrap_or(60);
    let trigger = match m.get("trigger").map(|s| s.as_str()) {
        Some("release") => Trigger::Release,
        _ => Trigger::Attack,
    };
    let pitch_keytrack = f("pitch_keytrack", 100.0) / 100.0;
    let on_locc64 = m.get("on_locc64").and_then(|v| v.parse::<i32>().ok());
    let kind = if let Some(lo) = on_locc64 {
        if lo >= 64 { RegionKind::PedalDown } else { RegionKind::PedalUp }
    } else if trigger == Trigger::Release {
        if pitch_keytrack == 0.0 { RegionKind::KeyNoise } else { RegionKind::DamperRelease }
    } else {
        RegionKind::Note
    };
    if lokey < 0 && on_locc64.is_none() {
        return Ok(None);
    }
    Ok(Some(Region {
        sample: sample.replace('\\', "/"),
        lokey: lokey.clamp(0, 127) as u8,
        hikey: hikey.clamp(0, 127) as u8,
        lovel: f("lovel", 1.0).clamp(1.0, 127.0) as u8,
        hivel: f("hivel", 127.0).clamp(1.0, 127.0) as u8,
        keycenter: keycenter.clamp(0, 127) as u8,
        pitch_keytrack,
        volume_db: f("volume", 0.0),
        amp_veltrack: f("amp_veltrack", 100.0) / 100.0,
        ampeg_release: f("ampeg_release", 0.001),
        rt_decay: f("rt_decay", 0.0),
        trigger,
        lorand: f("lorand", 0.0),
        hirand: f("hirand", 1.0),
        kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNIPPET: &str = r#"
//Notes
<group> amp_veltrack=73 ampeg_release=1

<region> sample=samples/A0v1.flac lokey=21 hikey=22 lovel=1 hivel=26 pitch_keycenter=21
<region> sample=samples/A0v16.flac lokey=21 hikey=22 lovel=121 pitch_keycenter=21
<group> trigger=release volume=-4 amp_veltrack=94 rt_decay=6
<region> sample=samples/harmLA0.flac lokey=20 hikey=22 lovel=45 pitch_keycenter=21
<group> trigger=release pitch_keytrack=0 volume=-37 amp_veltrack=82 rt_decay=2
<region> sample=samples/rel1.flac lokey=21 hikey=21
<group> group=1 hikey=-1 lokey=-1 on_locc64=126 on_hicc64=127 off_by=2 volume=-20
<region> sample=samples/pedalD1.flac lorand=0 hirand=0.5
"#;

    #[test]
    fn parses_salamander_style() {
        let r = parse(SNIPPET).unwrap();
        assert_eq!(r.len(), 5);
        assert_eq!(r[0].kind, RegionKind::Note);
        assert_eq!((r[0].lokey, r[0].hikey, r[0].lovel, r[0].hivel), (21, 22, 1, 26));
        assert!((r[0].amp_veltrack - 0.73).abs() < 1e-6);
        assert_eq!(r[1].hivel, 127);
        assert_eq!(r[2].kind, RegionKind::DamperRelease);
        assert_eq!(r[2].rt_decay, 6.0);
        assert_eq!(r[3].kind, RegionKind::KeyNoise);
        assert_eq!(r[3].volume_db, -37.0);
        assert_eq!(r[4].kind, RegionKind::PedalDown);
        assert_eq!(r[4].hirand, 0.5);
    }

    #[test]
    fn note_names() {
        assert_eq!(parse_key("c4"), Some(60));
        assert_eq!(parse_key("f#3"), Some(54));
        assert_eq!(parse_key("21"), Some(21));
    }
}
