//! SVG cards as plain string templates. Each card is (height, inner svg) at a fixed width;
//! `compose` places them in a 1- or 2-column grid inside one root `<svg>`.
//! All remote/user text goes through `esc`.

use std::fmt::Write;

use crate::db::{Card, Theme};
use crate::github::GithubData;
use crate::spotify::Track;

pub const W: u32 = 420;
const GAP: u32 = 10;

pub enum Data<'a> {
    Github(&'a GithubData),
    Languages(&'a GithubData),
    Streak(&'a GithubData),
    Spotify(Option<&'a Track>),
    /// A user's own card: (title, text).
    Custom(&'a str, &'a str),
    Error(Card, &'a str),
}

struct Palette {
    bg: &'static str,
    border: &'static str,
    title: &'static str,
    text: &'static str,
    muted: &'static str,
    ring_bg: &'static str,
}

fn palette(t: Theme) -> Palette {
    match t {
        Theme::Light => Palette { bg: "#fffefe", border: "#e4e2e2", title: "#2f80ed", text: "#434d58", muted: "#6a737d", ring_bg: "#e4e2e2" },
        Theme::Dark => Palette { bg: "#0d1117", border: "#30363d", title: "#58a6ff", text: "#c9d1d9", muted: "#8b949e", ring_bg: "#30363d" },
    }
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// Escaped and cut to `max` chars with an ellipsis.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        esc(s)
    } else {
        esc(&format!("{}…", s.chars().take(max - 1).collect::<String>()))
    }
}

fn fmt_num(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}m", n as f64 / 1e6),
    }
}

fn title(text: &str) -> String {
    format!(r#"<text x="25" y="35" class="t">{}</text>"#, clip(text, 38))
}

fn github(d: &GithubData, p: &Palette) -> (u32, String) {
    let rows = [
        ("Total Stars", d.stars),
        ("Commits (last year)", d.commits),
        ("Pull Requests", d.prs),
        ("Issues", d.issues),
        ("Followers", d.followers),
    ];
    let mut s = title(&format!("{}'s GitHub Stats", d.name));
    for (i, (label, v)) in rows.iter().enumerate() {
        let y = 70 + 25 * i as u32;
        let _ = write!(s, r#"<text x="25" y="{y}" class="l">{label}:</text><text x="200" y="{y}" class="v">{}</text>"#, fmt_num(*v));
    }
    // Ring fills as the percentile improves (lower is better).
    let (level, pct) = d.rank;
    let circ = 2.0 * std::f64::consts::PI * 40.0;
    let filled = circ * (100.0 - pct.clamp(0.0, 100.0)) / 100.0;
    let _ = write!(
        s,
        r#"<circle cx="335" cy="115" r="40" fill="none" stroke="{bg}" stroke-width="6"/><circle cx="335" cy="115" r="40" fill="none" stroke="{c}" stroke-width="6" stroke-linecap="round" stroke-dasharray="{filled:.1} {circ:.1}" transform="rotate(-90 335 115)"/><text x="335" y="124" text-anchor="middle" class="rank">{level}</text>"#,
        bg = p.ring_bg,
        c = p.title,
    );
    (195, s)
}

fn languages(d: &GithubData) -> (u32, String) {
    let mut s = title("Most Used Languages");
    let total: u64 = d.languages.iter().map(|l| l.2).sum();
    if total == 0 {
        s.push_str(r#"<text x="25" y="70" class="m">No languages found</text>"#);
        return (95, s);
    }
    let bar = (W - 50) as f64;
    let mut x = 25.0;
    s.push_str(r#"<clipPath id="lbar"><rect x="25" y="52" width="370" height="8" rx="4"/></clipPath><g clip-path="url(#lbar)">"#);
    for (_, colour, size) in &d.languages {
        let w = bar * *size as f64 / total as f64;
        let _ = write!(s, r#"<rect x="{x:.2}" y="52" width="{w:.2}" height="8" fill="{}"/>"#, esc(colour));
        x += w;
    }
    s.push_str("</g>");
    for (i, (name, colour, size)) in d.languages.iter().enumerate() {
        let (cx, y) = (25 + (i as u32 % 2) * 185, 88 + (i as u32 / 2) * 24);
        let _ = write!(
            s,
            r#"<circle cx="{}" cy="{}" r="5" fill="{}"/><text x="{}" y="{y}" class="l">{} <tspan class="m">{:.1}%</tspan></text>"#,
            cx + 5,
            y - 4,
            esc(colour),
            cx + 16,
            clip(name, 16),
            100.0 * *size as f64 / total as f64,
        );
    }
    let rows = (d.languages.len() as u32).div_ceil(2);
    (88 + 24 * rows, s)
}

fn streak(d: &GithubData) -> (u32, String) {
    let mut s = title("Contribution Streak");
    let cols = [
        (fmt_num(d.streak.total), "Contributions"),
        (d.streak.current.to_string(), "Current Streak"),
        (d.streak.longest.to_string(), "Longest Streak"),
    ];
    for (i, (value, label)) in cols.iter().enumerate() {
        let x = W / 6 + i as u32 * W / 3;
        let _ = write!(s, r#"<text x="{x}" y="88" text-anchor="middle" class="big">{value}</text><text x="{x}" y="115" text-anchor="middle" class="m">{label}</text>"#);
    }
    (140, s)
}

fn spotify(t: Option<&Track>, id: usize) -> (u32, String) {
    let Some(t) = t else {
        return (80, format!(r#"{}<text x="25" y="60" class="m">Nothing played recently</text>"#, title("Spotify")));
    };
    let mut s = String::new();
    match &t.art {
        Some(uri) => {
            let _ = write!(
                s,
                r#"<clipPath id="art{id}"><rect x="20" y="20" width="100" height="100" rx="8"/></clipPath><image x="20" y="20" width="100" height="100" href="{}" clip-path="url(#art{id})"/>"#,
                esc(uri)
            );
        }
        None => s.push_str(r#"<rect x="20" y="20" width="100" height="100" rx="8" class="ph"/>"#),
    }
    let status = if t.playing { "Now playing on Spotify" } else { "Last played on Spotify" };
    let _ = write!(
        s,
        r#"<text x="140" y="45" class="m">{status}</text><text x="140" y="78" class="track">{}</text><text x="140" y="102" class="l">{}</text>"#,
        clip(&t.name, 24),
        clip(&t.artists, 34),
    );
    (140, s)
}

const WRAP_CHARS: usize = 48;
const MAX_LINES: usize = 8;

/// Word-wraps `text` to lines of at most `max` chars, keeping line breaks; past `lines`
/// lines the last kept one ends in an ellipsis. Returns unescaped text.
// ponytail: counts chars, not pixels, so wide glyphs (CJK, emoji) can overflow; use font metrics if that matters.
fn wrap(text: &str, max: usize, lines: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.lines() {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let mut word = word.to_string();
            loop {
                let (len, wlen) = (line.chars().count(), word.chars().count());
                if len == 0 && wlen <= max {
                    line = word;
                } else if len > 0 && len + 1 + wlen <= max {
                    line.push(' ');
                    line.push_str(&word);
                } else if len > 0 {
                    out.push(std::mem::take(&mut line));
                    continue;
                } else {
                    // A word longer than a whole line: hard-break it.
                    out.push(word.chars().take(max).collect());
                    word = word.chars().skip(max).collect();
                    continue;
                }
                break;
            }
        }
        out.push(line);
    }
    if out.len() > lines {
        out.truncate(lines);
        let last = &mut out[lines - 1];
        *last = format!("{}…", last.chars().take(max - 1).collect::<String>());
    }
    out
}

fn custom(heading: &str, text: &str) -> (u32, String) {
    let mut s = title(heading);
    let lines = wrap(text, WRAP_CHARS, MAX_LINES);
    for (i, line) in lines.iter().enumerate() {
        let _ = write!(s, r#"<text x="25" y="{}" class="l">{}</text>"#, 65 + 20 * i as u32, esc(line));
    }
    (55 + 20 * lines.len() as u32, s)
}

fn error(card: Card, msg: &str) -> (u32, String) {
    let name = match card {
        Card::Github => "GitHub Stats",
        Card::Languages => "Most Used Languages",
        Card::Streak => "Contribution Streak",
        Card::Spotify => "Spotify",
    };
    (80, format!(r#"{}<text x="25" y="60" class="m">Unavailable: {}</text>"#, title(name), clip(msg, 48)))
}

pub fn compose(cards: &[Data], theme: Theme, columns: u8) -> String {
    let p = palette(theme);
    let cols: usize = if columns == 2 { 2 } else { 1 };
    let rendered: Vec<(u32, String)> = cards
        .iter()
        .enumerate()
        .map(|(i, c)| match c {
            Data::Github(d) => github(d, &p),
            Data::Languages(d) => languages(d),
            Data::Streak(d) => streak(d),
            Data::Spotify(t) => spotify(*t, i),
            Data::Custom(t, text) => custom(t, text),
            Data::Error(card, msg) => error(*card, msg),
        })
        .collect();

    let mut body = String::new();
    let mut y = 0;
    for row in rendered.chunks(cols) {
        let row_h = row.iter().map(|c| c.0).max().unwrap_or(0);
        for (i, (h, inner)) in row.iter().enumerate() {
            let x = i as u32 * (W + GAP);
            let _ = write!(
                body,
                r#"<svg x="{x}" y="{y}" width="{W}" height="{row_h}"><rect x="0.5" y="0.5" width="{}" height="{}" rx="6" class="card"/>{inner}</svg>"#,
                W - 1,
                h - 1,
            );
        }
        y += row_h + GAP;
    }
    let width = cols.min(rendered.len().max(1)) as u32 * (W + GAP) - GAP;
    let height = y.saturating_sub(GAP).max(1);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><style>
text{{font-family:'Segoe UI',Ubuntu,'Helvetica Neue',Arial,sans-serif;fill:{text}}}
.card{{fill:{bg};stroke:{border}}}
.ph{{fill:{border}}}
.t{{font-size:18px;font-weight:600;fill:{title}}}
.l{{font-size:14px}}
.v{{font-size:14px;font-weight:700}}
.m{{font-size:13px;fill:{muted}}}
.big{{font-size:28px;font-weight:700;fill:{title}}}
.track{{font-size:17px;font-weight:700}}
.rank{{font-size:26px;font-weight:800}}
</style>{body}</svg>"#,
        text = p.text,
        bg = p.bg,
        border = p.border,
        title = p.title,
        muted = p.muted,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Streak;

    fn data() -> GithubData {
        GithubData {
            name: "<script>".into(),
            stars: 1500,
            commits: 3,
            prs: 0,
            issues: 0,
            followers: 0,
            rank: ("B", 60.0),
            languages: vec![("Rust".into(), "#dea584".into(), 3), ("Go".into(), "#00ADD8".into(), 1)],
            streak: Streak { total: 3, current: 1, longest: 2 },
        }
    }

    #[test]
    fn escapes_text() {
        assert_eq!(esc(r#"<a href="x">&'"#), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
        let d = data();
        let svg = compose(&[Data::Github(&d)], Theme::Light, 1);
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;"));
        assert!(svg.contains("1.5k"));
    }

    #[test]
    fn grid_dimensions() {
        let d = data();
        let one = compose(&[Data::Github(&d), Data::Streak(&d)], Theme::Dark, 1);
        assert!(one.contains(r#"width="420" height="345""#), "{one}"); // 195 + 10 + 140
        let two = compose(&[Data::Github(&d), Data::Streak(&d), Data::Error(Card::Spotify, "x")], Theme::Dark, 2);
        assert!(two.contains(r#"width="850" height="285""#)); // row max 195 + 10 + 80
        let lone = compose(&[Data::Spotify(None)], Theme::Light, 2);
        assert!(lone.contains(r#"width="420" height="80""#));
    }

    #[test]
    fn wraps_text() {
        assert_eq!(wrap("", 10, 3), Vec::<String>::new());
        assert_eq!(wrap("aa bb cc", 5, 3), ["aa bb", "cc"]);
        assert_eq!(wrap("a

b", 5, 3), ["a", "", "b"]);
        assert_eq!(wrap("abcdefghijkl x", 5, 9), ["abcde", "fghij", "kl x"]);
        assert_eq!(wrap("a b c d", 1, 3), ["a", "b", "…"]);
        assert_eq!(wrap("aaaaa bbbbb ccccc", 5, 2), ["aaaaa", "bbbb…"]);
    }

    #[test]
    fn custom_card() {
        let svg = compose(&[Data::Custom("<b>Me</b>", "hi <script>
there")], Theme::Light, 1);
        assert!(!svg.contains("<script>") && !svg.contains("<b>"));
        assert!(svg.contains("hi &lt;script&gt;"));
        assert!(svg.contains(r#"width="420" height="95""#), "{svg}"); // 55 + 2 lines
    }

    #[test]
    fn clips_long_text() {
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("ab", 4), "ab");
    }
}
